#ifndef SIMPLENOTORIOUS_H
#define SIMPLENOTORIOUS_H

/* C ABI of crates/ffi. All calls take/return UTF-8 JSON:
 * {"ok": ...} on success, {"error": "..."} on failure.
 * Strings returned by sn_call must be freed with sn_free_string. */
typedef struct SnCore SnCore;
typedef void (*SnEventCallback)(const char *event_json, void *user);

SnCore *sn_open(const char *config_json);
void sn_close(SnCore *core);
void sn_free_string(char *s);
void sn_set_callback(SnCore *core, SnEventCallback cb, void *user);
char *sn_call(SnCore *core, const char *method, const char *args_json);

#endif
