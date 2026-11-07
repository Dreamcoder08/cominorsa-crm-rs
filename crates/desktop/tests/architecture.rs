//! The desktop calls the application layer, so a flow cannot grow a second
//! path that the CLI never proves.

use std::{fs, path::Path};

/// Repository writes and use-case steps a view must never call itself.
const FORBIDDEN_CALLS: &[&str] = &[
    // Repository writes.
    ".save(",
    ".save_step(",
    ".persist(",
    ".sign_and_open(",
    ".close_case(",
    ".save_contract_total(",
    ".add_milestones(",
    ".save_milestone(",
    ".delete_milestone(",
    ".create_quote(",
    ".save_quote_draft(",
    ".save_quote_status(",
    ".backup_to(",
    // Steps that the use cases compose.
    "parse_form(",
    "billing_change(",
    "save_billing(",
    "quote_change(",
    "save_quote(",
    "create_backup(",
    "quote_pdf(",
    "write_pdf(",
    "toggled_step(",
    "parse_step_date(",
    ".assign(",
    ".reschedule(",
];

/// Whether `line` calls `call`, not a longer name that ends the same way.
fn calls(line: &str, call: &str) -> bool {
    line.match_indices(call).any(|(start, _)| {
        call.starts_with('.')
            || !line[..start]
                .chars()
                .next_back()
                .is_some_and(|before| before == '_' || before.is_alphanumeric())
    })
}

fn violations(source: &str) -> Vec<&'static str> {
    FORBIDDEN_CALLS
        .iter()
        .copied()
        .filter(|call| {
            source.lines().any(|line| {
                let code = line.split("//").next().unwrap_or_default();
                calls(code, call)
            })
        })
        .collect()
}

#[test]
fn the_check_notices_a_direct_repository_write() {
    assert_eq!(violations("repository.save(&lead).await?;"), [".save("]);
    assert!(violations("// repository.save(&lead)").is_empty());
    assert!(violations("save_quote_pdf(&repository, &quote, target)").is_empty());
}

#[test]
fn the_desktop_never_composes_a_flow_owned_by_a_use_case() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    for entry in fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let found = violations(&fs::read_to_string(&path).unwrap());
            assert!(found.is_empty(), "{}: {found:?}", path.display());
        }
    }
}
