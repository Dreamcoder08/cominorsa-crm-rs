//! An id seed names one request. Reusing it for anything else is refused.

mod common;

use common::{
    Scratch, add_lead, data, export, lead_about_to_sign, open_case, refused, run, seeded, text,
};
use serde_json::Value;

const EXTRA: &[&str] = &["--title", "Extra", "--amount", "100", "--due", "2026-05-01"];

fn billing_add<'a>(case: &'a str, fields: &'a [&'a str]) -> Vec<&'a str> {
    let mut args = vec!["billing", "add", case];
    args.extend_from_slice(fields);
    args
}

fn sent_quote(db: &std::path::Path, lead: &str, line: &str) -> String {
    let quote = data(&run(
        db,
        &["quote", "create", "--lead", lead, "--line", line],
    ));
    let number = quote["quote"]["number"].as_str().unwrap().to_owned();
    data(&run(
        db,
        &["quote", "send", &number, "--allow-blank-company"],
    ));
    number
}

#[test]
fn billing_add_refuses_a_seed_with_other_data() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let case = open_case(&db, "Uno", "987654321", "REINFO");
    let id = text(&case["id"]);
    data(&seeded(&db, "b1", &billing_add(id, EXTRA)));
    let before = export(&db);

    let other = ["--title", "Otro", "--amount", "999", "--due", "2026-06-01"];
    let output = seeded(&db, "b1", &billing_add(id, &other));

    refused(&db, output, 1, "id_seed_reused", &before);
}

#[test]
fn billing_add_repeats_the_same_request_without_a_change() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let case = open_case(&db, "Uno", "987654321", "REINFO");
    let id = text(&case["id"]);
    data(&seeded(&db, "b1", &billing_add(id, EXTRA)));
    let before = export(&db);

    let again = data(&seeded(&db, "b1", &billing_add(id, EXTRA)));

    assert_eq!(again["changed"], false);
    assert_eq!(export(&db), before);
}

#[test]
fn billing_add_refuses_a_seed_used_on_another_case() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let first = open_case(&db, "Uno", "987654321", "REINFO");
    let second = open_case(&db, "Dos", "987654322", "REINFO");
    data(&seeded(&db, "b1", &billing_add(text(&first["id"]), EXTRA)));
    let before = export(&db);

    let output = seeded(&db, "b1", &billing_add(text(&second["id"]), EXTRA));

    refused(&db, output, 1, "id_seed_reused", &before);
}

#[test]
fn billing_add_refuses_a_seed_held_by_a_settled_case() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let settled = open_case(&db, "Uno", "987654321", "REINFO");
    let open = open_case(&db, "Dos", "987654322", "REINFO");
    let id = text(&settled["id"]);
    let added = data(&seeded(&db, "b1", &billing_add(id, EXTRA)));
    data(&run(
        &db,
        &["billing", "pay", text(&added["milestones"][0]["id"])],
    ));
    data(&run(&db, &["case", "close", id]));
    let before = export(&db);

    let output = seeded(&db, "b1", &billing_add(text(&open["id"]), EXTRA));

    refused(&db, output, 1, "id_seed_reused", &before);
}

#[test]
fn quote_version_refuses_a_seed_that_names_another_quote() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_lead(&db, "Uno", "987654321", "DAC");
    let first = sent_quote(&db, &lead, "a|1|100");
    let second = sent_quote(&db, &lead, "b|1|200");
    data(&seeded(&db, "zz", &["quote", "version", &first]));
    let before = export(&db);

    let output = seeded(&db, "zz", &["quote", "version", &second]);

    refused(&db, output, 1, "id_seed_reused", &before);
}

#[test]
fn quote_version_repeats_the_same_request_without_a_change() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_lead(&db, "Uno", "987654321", "DAC");
    let first = sent_quote(&db, &lead, "a|1|100");
    let made = data(&seeded(&db, "zz", &["quote", "version", &first]));
    let before = export(&db);

    let again = data(&seeded(&db, "zz", &["quote", "version", &first]));

    assert_eq!(again["quote"]["number"], made["quote"]["number"]);
    assert_eq!(export(&db), before);
}

#[test]
fn closing_a_case_refuses_a_seed_that_names_another_lead() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let case = open_case(&db, "Uno", "987654321", "DAC");
    data(&seeded(
        &db,
        "z0",
        &[
            "lead",
            "add",
            "--name",
            "Zeta",
            "--phone",
            "987654332",
            "--service",
            "REINFO",
            "--follow-up",
            "2026-04-02",
        ],
    ));
    let before = export(&db);

    let output = seeded(&db, "z0", &["case", "close", text(&case["id"])]);

    refused(&db, output, 1, "id_seed_reused", &before);
}

#[test]
fn closing_a_case_again_with_its_seed_changes_nothing() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let case = open_case(&db, "Uno", "987654321", "DAC");
    let id = text(&case["id"]);
    let first = data(&seeded(&db, "c1", &["case", "close", id]));
    assert_eq!(first["closed"], true);
    let before = export(&db);

    let again = data(&seeded(&db, "c1", &["case", "close", id]));

    assert_eq!(again["closed"], false);
    assert_eq!(again["renewal"], Value::Null);
    assert_eq!(export(&db), before);
}

#[test]
fn opening_a_case_refuses_a_seed_held_by_another_case() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let first = lead_about_to_sign(&db, "Uno", "987654321", "REINFO");
    let second = lead_about_to_sign(&db, "Dos", "987654322", "REINFO");
    data(&seeded(&db, "cs", &["lead", "advance", &first]));
    let before = export(&db);

    let output = seeded(&db, "cs", &["lead", "advance", &second]);

    refused(&db, output, 1, "id_seed_reused", &before);
}

#[test]
fn lead_add_and_quote_create_share_the_code() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = |name: &'static str| {
        vec![
            "lead",
            "add",
            "--name",
            name,
            "--phone",
            "987654331",
            "--service",
            "DAC",
            "--follow-up",
            "2026-04-01",
        ]
    };
    let added = data(&seeded(&db, "a1", &lead("U")));
    let id = text(&added["lead"]["id"]).to_owned();
    let create = |line: &'static str| {
        vec!["quote", "create", "--lead", &id, "--line", line]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let first: Vec<String> = create("x|1|100");
    let first: Vec<&str> = first.iter().map(String::as_str).collect();
    data(&seeded(&db, "q1", &first));
    let before = export(&db);

    refused(
        &db,
        seeded(&db, "a1", &lead("V")),
        1,
        "id_seed_reused",
        &before,
    );
    let other: Vec<String> = create("y|1|222");
    let other: Vec<&str> = other.iter().map(String::as_str).collect();
    refused(&db, seeded(&db, "q1", &other), 1, "id_seed_reused", &before);
}
