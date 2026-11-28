#!/usr/bin/env nu
# Regenerate crates/linux/data/licenses.json (third-party licenses shown in the About dialog).
#   needs: cargo install cargo-about --locked --features cli
# Re-run whenever the Linux app's dependencies change.

def main [] {
    let linux = ($env.FILE_PWD | path dirname | path join crates linux)
    let raw = (mktemp --suffix .json)

    cd $linux
    ^cargo about generate --format json --output-file $raw

    let entries = (
        open $raw
        | get licenses
        | each {|lic|
            $lic.used_by | each {|u|
                {
                    name: $u.crate.name
                    version: $u.crate.version
                    authors: ($u.crate.authors | str join ", ")
                    description: ($u.crate.description | default "" | str trim)
                    license_id: $lic.id
                    license_text: $lic.text
                }
            }
        }
        | flatten
        | where {|e| not ($e.name | str starts-with "simplenotorious-") }
        | group-by {|e| $"($e.name)@($e.version)" }
        | transpose key rows
        | each {|g|
            let rows = $g.rows
            ($rows | first) | merge {
                license_id: ($rows | get license_id | uniq | str join " AND ")
                license_text: ($rows | get license_text | uniq | str join "\n\n----------\n\n")
            }
        }
        | sort-by {|e| $e.name | str lowercase } version
    )

    rm $raw
    $entries | to json | save --force ($linux | path join data licenses.json)
    print $"Wrote ($entries | length) licenses to crates/linux/data/licenses.json"
}
