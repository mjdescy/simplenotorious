//! C ABI for the SwiftUI app. Every call takes/returns UTF-8 JSON:
//! `{"ok": <value>}` on success or `{"error": "<message>"}` on failure.
//! Strings returned by this library must be released with `sn_free_string`.
use serde_json::{Value, json};
use simplenotorious_core::{Core, Event, Filter, Paths, Settings};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::PathBuf,
};

struct UserData(*mut c_void);
unsafe impl Send for UserData {}
unsafe impl Sync for UserData {}

pub type EventCallback = extern "C" fn(event_json: *const c_char, user: *mut c_void);

fn to_c(v: Value) -> *mut c_char {
    CString::new(v.to_string().replace('\0', "")).unwrap().into_raw()
}

unsafe fn from_c<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
    }
}

/// `config_json` may be `{}` or `{"config_dir": "...", "data_dir": "..."}`.
/// Returns NULL on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sn_open(config_json: *const c_char) -> *mut Core {
    let cfg: Value = serde_json::from_str(unsafe { from_c(config_json) }).unwrap_or(json!({}));
    let mut paths = Paths::default_dirs();
    if let Some(p) = cfg["config_dir"].as_str() {
        paths.config_dir = PathBuf::from(p);
    }
    if let Some(p) = cfg["data_dir"].as_str() {
        paths.data_dir = PathBuf::from(p);
    }
    match Core::open(paths) {
        Ok(c) => Box::into_raw(Box::new(c)),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sn_close(core: *mut Core) {
    if !core.is_null() {
        let c = unsafe { Box::from_raw(core) };
        c.stop_background();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sn_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Register an event callback (called from a background thread). Events:
/// `{"event":"notes_changed"}` and `{"event":"status","status":{...}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sn_set_callback(core: *mut Core, cb: EventCallback, user: *mut c_void) {
    let core = unsafe { &*core };
    let user = UserData(user);
    core.set_listener(move |e| {
        let v = match e {
            Event::NotesChanged => json!({"event": "notes_changed"}),
            Event::StatusChanged(s) => json!({"event": "status", "status": s}),
        };
        let _user = &user;
        if let Ok(c) = CString::new(v.to_string()) {
            cb(c.as_ptr(), user.0);
        }
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sn_call(core: *mut Core, method: *const c_char, args_json: *const c_char) -> *mut c_char {
    let core = unsafe { &*core };
    let args: Value = serde_json::from_str(unsafe { from_c(args_json) }).unwrap_or(Value::Null);
    match dispatch(core, unsafe { from_c(method) }, &args) {
        Ok(v) => to_c(json!({"ok": v})),
        Err(e) => to_c(json!({"error": e})),
    }
}

fn s<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a[k].as_str().ok_or_else(|| format!("missing argument '{k}'"))
}

fn ser<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn dispatch(c: &Core, method: &str, a: &Value) -> Result<Value, String> {
    let e = |e: simplenotorious_core::Error| e.to_string();
    match method {
        "list" => {
            let f = Filter {
                query: a["query"].as_str().map(str::to_string),
                tag: a["tag"].as_str().map(str::to_string),
                trash: a["trash"].as_bool().unwrap_or(false),
                ..Default::default()
            };
            ser(c.list(&f).map_err(e)?)
        }
        "tags" => ser(c.tags().map_err(e)?),
        "get" => ser(c.get(s(a, "id")?).map_err(e)?),
        "create_note" => ser(c.create_note().map_err(e)?),
        "update_content" => ser(c.update_content(s(a, "id")?, s(a, "content")?).map_err(e)?),
        "set_pinned" => ser(c
            .set_pinned(s(a, "id")?, a["pinned"].as_bool().unwrap_or(false))
            .map_err(e)?),
        "set_tags" => {
            let tags = a["tags"]
                .as_array()
                .map(|v| v.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            ser(c.set_tags(s(a, "id")?, tags).map_err(e)?)
        }
        "trash" => c.trash(s(a, "id")?).map(|_| Value::Null).map_err(e),
        "restore" => c.restore(s(a, "id")?).map(|_| Value::Null).map_err(e),
        "preview" => Ok(json!(
            c.render_preview(s(a, "id")?, a["dark"].as_bool().unwrap_or(false))
                .map_err(e)?
        )),
        "settings" => ser(c.settings()),
        "update_settings" => {
            let mut cur = ser(c.settings())?;
            if let (Some(cur), Some(patch)) = (cur.as_object_mut(), a.as_object()) {
                for (k, v) in patch {
                    cur.insert(k.clone(), v.clone());
                }
            }
            let new: Settings = serde_json::from_value(cur).map_err(|x| x.to_string())?;
            ser(c.update_settings(|s| *s = new).map_err(e)?)
        }
        "is_logged_in" => Ok(json!(c.is_logged_in())),
        "login_url" => Ok(json!(c.login_url())),
        "login" => c.login(s(a, "credential")?).map(|_| Value::Null).map_err(e),
        "logout" => c
            .logout(a["delete_local_notes"].as_bool().unwrap_or(false))
            .map(|_| Value::Null)
            .map_err(e),
        "status" => ser(c.status()),
        "last_sync" => Ok(json!(c.last_sync_ms())),
        "sync_now" => {
            c.sync_now();
            Ok(Value::Null)
        }
        "start_background" => {
            c.start_background();
            Ok(Value::Null)
        }
        "notes_folder" => Ok(json!(c.notes_folder())),
        "json_path" => Ok(json!(c.json_path())),
        "import_json" => Ok(json!(c.import_json(s(a, "json")?).map_err(e)?)),
        "export_json" => c
            .export_json_to(std::path::Path::new(s(a, "path")?))
            .map(|_| Value::Null)
            .map_err(e),
        other => Err(format!("unknown method '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let d = std::env::temp_dir().join(format!("sn-ffi-{}", std::process::id()));
        let cfg = CString::new(json!({"config_dir": d.join("c"), "data_dir": d.join("d")}).to_string()).unwrap();
        unsafe {
            let core = sn_open(cfg.as_ptr());
            assert!(!core.is_null());
            let call = |m: &str, a: Value| -> Value {
                let m = CString::new(m).unwrap();
                let a = CString::new(a.to_string()).unwrap();
                let p = sn_call(core, m.as_ptr(), a.as_ptr());
                let v: Value = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
                sn_free_string(p);
                v
            };
            let n = call("create_note", json!({}));
            let id = n["ok"]["id"].as_str().unwrap().to_string();
            call("update_content", json!({"id": id, "content": "# Hi"}));
            let l = call("list", json!({}));
            assert_eq!(l["ok"][0]["title"], "Hi");
            assert_eq!(call("status", json!({}))["ok"]["state"], "logged_out");
            assert!(call("nope", json!({}))["error"].is_string());
            let st = call("update_settings", json!({"theme": "dark"}));
            assert_eq!(st["ok"]["theme"], "dark");
            sn_close(core);
        }
        let _ = std::fs::remove_dir_all(d);
    }
}
