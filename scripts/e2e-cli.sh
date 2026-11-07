#!/usr/bin/env bash
# End-to-end proof through the `cominorsa` CLI alone, on synthetic data in a
# fresh temporary database. Usage: scripts/e2e-cli.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# A missing assertion tool must fail the gate, never skip it.
for tool in jq sha256sum sqlite3; do
    if ! command -v "$tool" >/dev/null; then
        echo "FAILED: $tool is required to assert the scenario" >&2
        exit 1
    fi
done

cargo build --quiet --locked -p cominorsa-cli
binary="$PWD/target/debug/cominorsa"

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT

day="2026-03-02"
checks=0
db="crm.db"
no_id="00000000-0000-0000-0000-000000000000"

# Runs one command on a fixed day and prints its JSON data. The id seed comes
# from the command itself, so a repeated command repeats its ids.
cli() {
    local seed
    seed="$(printf '%s' "$*" | sha256sum | cut -c1-16)"
    "$binary" --db "$db" --json --today "$day" --id-seed "$seed" "$@" | jq -c '.data'
}

fail() {
    echo "FAILED: $1" >&2
    exit 1
}

# expect <description> <expected> <actual>
expect() {
    checks=$((checks + 1))
    [[ "$2" == "$3" ]] || fail "$1: expected '$2', got '$3'"
}

state_hash() {
    "$binary" --db "$db" --json export | sha256sum | cut -d' ' -f1
}

file_hash() {
    sha256sum "$1" | cut -d' ' -f1
}

# read_only <args...>: the command succeeds and the database file is untouched.
read_only() {
    local before
    before="$(file_hash "$db")"
    "$binary" --db "$db" --json --today "$day" "$@" >/dev/null
    expect "file hash after '$*'" "$before" "$(file_hash "$db")"
    # SQLite may leave an empty WAL beside the file; content would be a write.
    expect "WAL after '$*'" 0 "$(wc -c <"$db-wal" 2>/dev/null || echo 0)"
}

# refused <exit> <code> <args...>: the command fails with that exit code and
# error code, says so only on stderr, and the exported state stays identical.
refused() {
    local exit="$1" code="$2"
    shift 2
    local before out err status=0
    before="$(state_hash)"
    out="$scratch/refused.out"
    err="$scratch/refused.err"
    "$binary" --db "$db" --json --today "$day" "$@" >"$out" 2>"$err" || status=$?

    expect "exit of '$*'" "$exit" "$status"
    expect "stdout of '$*'" "" "$(cat "$out")"
    expect "error code of '$*'" "$code" "$(jq -r '.error.code' "$err")"
    expect "state hash after '$*'" "$before" "$(state_hash)"
}

# refused_absent <args...>: a write on a database that does not exist is refused
# and creates nothing.
refused_absent() {
    local status=0
    "$binary" --db "$db" --json --today "$day" "$@" >absent.out 2>absent.err || status=$?
    expect "exit of '$*' without a database" 3 "$status"
    expect "stdout of '$*' without a database" "" "$(cat absent.out)"
    expect "code of '$*' without a database" database_missing "$(jq -r '.error.code' absent.err)"
    [[ ! -e "$db" ]] || fail "'$*' created the database"
    rm -f absent.out absent.err
}

# refused_on <file> <code> <args...>: a write on a schema it cannot use is
# refused and the file keeps its hash.
refused_on() {
    local file="$1" code="$2"
    shift 2
    local before status=0
    before="$(file_hash "$file")"
    "$binary" --db "$file" --json --today "$day" "$@" >write.out 2>write.err || status=$?
    expect "exit of '$*' on $file" 3 "$status"
    expect "stdout of '$*' on $file" "" "$(cat write.out)"
    expect "code of '$*' on $file" "$code" "$(jq -r '.error.code' write.err)"
    expect "file hash after '$*' on $file" "$before" "$(file_hash "$file")"
}

# Writes that must never reach an older or newer schema.
refused_writes() {
    local file="$1" code="$2"
    refused_on "$file" "$code" lead add --name "" --phone 987654321 --service DAC
    refused_on "$file" "$code" lead advance "$no_id"
    refused_on "$file" "$code" case close "$no_id"
    refused_on "$file" "$code" step done "$no_id"
    refused_on "$file" "$code" billing pay "$no_id"
    refused_on "$file" "$code" quote accept COT-1999-0001
    refused_on "$file" "$code" settings set --follow-up-days 5
    refused_on "$file" "$code" prospect import --companies none.csv --people none.csv
    refused_on "$file" "$code" backup create --folder copias-viejas
}

# The whole sale. Paths in the data stay relative, so two runs store the same bytes.
scenario() {
    scratch="$PWD"
    echo "== a write on a missing database creates nothing"
    refused_absent lead add --name "" --phone 987654321 --service DAC
    refused_absent lead advance "$no_id"
    refused_absent settings set --theme dark
    refused_absent prospect import --companies none.csv --people none.csv
    expect "init creates the database" true "$(cli init | jq '.created')"
    expect "init schema" 8 "$(cli status | jq '.schema_version')"
    refused 1 database_exists init

    echo "== team and settings"
    cli settings set \
        --company-name "Consultora Sintética SAC" --company-ruc 20100070970 \
        --add-member "Ana Prueba" --add-member "Luis Prueba" \
        --follow-up-days 7 --igv 18 --backup-directory copias >/dev/null
    shown="$(cli settings show)"
    expect "team" '["Ana Prueba","Luis Prueba"]' "$(jq -c '.team' <<<"$shown")"
    expect "company" "Consultora Sintética SAC" "$(jq -r '.company.name' <<<"$shown")"

    echo "== prospects through the existing importer"
    header="titular,perfil_icp,n_concesiones,hectareas_totales,departamentos,provincias,sustancias,estados_concesion,servicio_potencial,revisar_manual,fuente_dato"
    cat >"$scratch/companies.csv" <<CSV
$header
Minera Sintética SAC,PERFIL_1_formalizacion,3,120.5,PIURA,SULLANA,M,TITULADA,IGAFOM + DAC,FALSE,Synthetic
Compañía Ficticia SAC,PERFIL_2_cumplimiento,2,40,LIMA,CANTA,M,TITULADA,DAC,FALSE,Synthetic
CSV
    cat >"$scratch/people.csv" <<CSV
$header
Persona Ficticia,PERFIL_2_cumplimiento,1,10,LIMA,HUAROCHIRI,M,TITULADA,DAC,FALSE,Synthetic
CSV
    import=(prospect import --companies "$scratch/companies.csv" --people "$scratch/people.csv")
    expect "imported" 3 "$(cli "${import[@]}" | jq '.added')"
    hash_before="$(state_hash)"
    expect "a repeated import adds nothing" 0 "$(cli "${import[@]}" | jq '.added')"
    expect "state after a repeated import" "$hash_before" "$(state_hash)"
    expect "prospects" 3 "$(cli prospect count | jq '.total')"

    echo "== convert a prospect into a lead"
    convert=(prospect convert --name "Minera Sintética SAC" --phone 987654321 --owner "Ana Prueba")
    lead="$(cli "${convert[@]}" | jq -r '.lead.id')"
    expect "lead service" "IGAFOM + DAC" "$(cli lead show "$lead" | jq -r '.lead.service')"
    expect "lead follow-up" "$day" "$(cli lead show "$lead" | jq -r '.lead.follow_up_on')"
    expect "repeating a seeded conversion creates nothing" false "$(cli "${convert[@]}" | jq '.created')"
    expect "leads" 1 "$(cli lead list | jq '.leads | length')"
    url="$(cli lead whatsapp "$lead" | jq -r '.url')"
    expect "only a prepared WhatsApp URL" "https://wa.me/51987654321" "${url%%\?*}"

    echo "== quote: create, send, accept, PDF"
    quote="$(cli quote create --lead "$lead" --line "Estudio de coordenadas|2|500.00" --line "Trámite DAC|1|1000.00" --notes "Sintética" | jq -r '.quote.number')"
    expect "quote number" "COT-2026-0001" "$quote"
    create_quote=(quote create --lead "$lead" --line "Estudio de coordenadas|2|500.00" --line "Trámite DAC|1|1000.00" --notes "Sintética")
    hash_before="$(state_hash)"
    expect "a repeated seeded quote keeps its number" "$quote" "$(cli "${create_quote[@]}" | jq -r '.quote.number')"
    expect "state after a repeated quote" "$hash_before" "$(state_hash)"
    expect "quote counter" '[{"last_sequence":1,"year":2026}]' "$(cli export | jq -cS '.quote_counters')"
    refused 1 invalid_quote_transition quote accept "$quote"
    sent="$(cli quote send "$quote")"
    expect "quote sent" sent "$(jq -r '.quote.status' <<<"$sent")"
    expect "company frozen" "Consultora Sintética SAC" "$(jq -r '.quote.company.name' <<<"$sent")"
    refused 1 quote_locked quote edit "$quote" --notes "Cambio"
    expect "quote accepted" accepted "$(cli quote accept "$quote" | jq -r '.quote.status')"
    total="$(cli quote show "$quote" | jq '.quote.totals.total_cents')"
    expect "quote total with IGV" 236000 "$total"

    pdf="$(cli quote pdf "$quote" --out "$scratch/cotizacion.pdf" | jq -r '.path')"
    expect "PDF signature" "%PDF-" "$(head -c 5 "$pdf")"
    if command -v pdftotext >/dev/null; then
        pdf_text="$(pdftotext "$pdf" -)"
        grep -q "$quote" <<<"$pdf_text" || fail "the PDF does not show $quote"
        grep -q "Minera Sintética SAC" <<<"$pdf_text" || fail "the PDF lost the client's accents"
        grep -q "no es un comprobante de pago" <<<"$pdf_text" || fail "the PDF lost its tax note"
        checks=$((checks + 3))
    else
        echo "SKIPPED: PDF text check, pdftotext is not installed"
    fi

    echo "== advance to Contrato firmado"
    for stage in coordinate_evaluation quote_sent contract_signed; do
        moved="$(cli lead advance "$lead")"
        expect "stage" "$stage" "$(jq -r '.lead.stage' <<<"$moved")"
    done
    case="$(jq -r '.case.id' <<<"$moved")"
    expect "exactly one case" 1 "$(cli case list | jq '.cases | length')"
    expect "accepted quote suggests the total" "$total" "$(jq '.case.contract_total_cents' <<<"$moved")"
    titles="$(jq -c '[.case.steps[].title | split(":")[0]] | unique' <<<"$moved")"
    expect "combined steps" '["DAC","IGAFOM"]' "$titles"
    expect "steps keep their order" "$(jq -c '[.case.steps[].position] | sort' <<<"$moved")" "$(jq -c '[.case.steps[].position]' <<<"$moved")"
    refused 1 final_stage lead advance "$lead"
    expect "still one case" 1 "$(cli case list | jq '.cases | length')"

    echo "== mark steps and edit a date"
    first_step="$(jq -r '.case.steps[0].id' <<<"$moved")"
    second_step="$(jq -r '.case.steps[1].id' <<<"$moved")"
    expect "step done" true "$(cli step done "$first_step" | jq '.changed')"
    expect "repeating a step changes nothing" false "$(cli step done "$first_step" | jq '.changed')"
    expect "new date" "2026-03-04" "$(cli step date "$second_step" --date 2026-03-04 | jq -r '.step.due_on')"
    refused 1 invalid_date step date "$second_step" --date mañana
    done_on="$(cli case show "$case" | jq -r --arg id "$first_step" '.case.steps[] | select(.id == $id) | .done_on')"
    expect "step done date" "$day" "$done_on"

    echo "== billing: 30 / 40 / 30"
    cli billing total "$case" --amount 10000.00 >/dev/null
    plan="$(cli billing plan "$case")"
    expect "plan amounts" "[300000,400000,300000]" "$(jq -c '[.milestones[].amount_cents]' <<<"$plan")"
    refused 1 milestones_already_planned billing plan "$case"
    first_milestone="$(jq -r '.milestones[0].id' <<<"$plan")"
    expect "paid" true "$(cli billing pay "$first_milestone" | jq '.changed')"
    expect "paying twice changes nothing" false "$(cli billing pay "$first_milestone" | jq '.changed')"
    billing="$(cli case show "$case" | jq -c '.billing')"
    expect "collected" 300000 "$(jq '.collected_cents' <<<"$billing")"
    expect "pending" 700000 "$(jq '.pending_cents' <<<"$billing")"
    expect "no gap" null "$(jq '.gap_warning' <<<"$billing")"

    echo "== agenda at fixed dates"
    agenda_on() {
        "$binary" --db "$db" --json --today "$1" agenda | jq -c '.data'
    }
    milestone_due="$(jq -r '.milestones[1].due_on' <<<"$plan")"
    step_item='[.due_today[] | select(.kind == "step" and .step_id == $id)] | length'
    expect "edited step is due that day" 1 "$(agenda_on 2026-03-04 | jq --arg id "$second_step" "$step_item")"
    expect "unpaid milestone is due that day" 1 \
        "$(agenda_on "$milestone_due" | jq '[.due_today[] | select(.kind == "milestone")] | length')"
    late="$(date -d "$milestone_due + 1 day" +%F 2>/dev/null || date -v+1d -j -f %F "$milestone_due" +%F)"
    expect "unpaid milestone turns overdue" 1 \
        "$(agenda_on "$late" | jq --arg due "$milestone_due" '[.overdue[] | select(.kind == "milestone" and .date == $due)] | length')"
    expect "a paid milestone leaves the agenda" 0 \
        "$(agenda_on "$late" | jq --arg id "$first_milestone" '[.overdue[], .due_today[], .upcoming[] | select(.milestone_id == $id)] | length')"

    echo "== close a recurring case"
    closed="$(cli case close "$case")"
    renewal="$(jq -r '.renewal.id' <<<"$closed")"
    expect "closed" true "$(jq '.closed' <<<"$closed")"
    expect "renewal in the first stage" initial_contact "$(jq -r '.renewal.stage' <<<"$closed")"
    expect "renewal follow-up" "2027-03-02" "$(jq -r '.renewal.follow_up_on' <<<"$closed")"
    hash_before="$(state_hash)"
    expect "a repeated close changes nothing" false "$(cli case close "$case" | jq '.closed')"
    expect "state after a repeated close" "$hash_before" "$(state_hash)"
    renewals="$(cli export | jq --arg case "$case" '[.leads[] | select(.renewal_of == $case)] | length')"
    expect "exactly one renewal" 1 "$renewals"
    expect "the renewal is a lead" "$renewal" "$(cli lead show "$renewal" | jq -r '.lead.id')"
    expect "money still owed keeps the case listed" 1 "$(cli case list | jq '.cases | length')"

    echo "== a settled case stays visible"
    one_off="$(cli lead add --name "Cliente Único" --phone 987654323 --service REINFO | jq -r '.lead.id')"
    cli lead advance "$one_off" >/dev/null
    cli lead advance "$one_off" >/dev/null
    settled="$(cli lead advance "$one_off" | jq -r '.case.id')"
    cli case close "$settled" >/dev/null
    expect "a settled case can be shown" closed "$(cli case show "$settled" | jq -r '.case.status')"
    expect "a settled case leaves the list" 0 \
        "$(cli case list | jq --arg id "$settled" '[.cases[] | select(.id == $id)] | length')"
    expect "export keeps the settled case" closed \
        "$(cli export | jq -r --arg id "$settled" '.cases[] | select(.id == $id) | .status')"
    hash_before="$(state_hash)"
    expect "closing a settled case again changes nothing" false "$(cli case close "$settled" | jq '.closed')"
    expect "state after closing a settled case again" "$hash_before" "$(state_hash)"
    refused 1 case_not_found case show 00000000-0000-0000-0000-000000000000

    echo "== refused commands leave the state identical"
    refused 1 missing_field lead add --name "" --phone 987654321 --service DAC
    refused 1 invalid_ruc lead add --name X --phone 987654321 --service DAC --ruc 123
    refused 1 lead_not_found lead advance 00000000-0000-0000-0000-000000000000
    refused 1 duplicate_member settings set --add-member "Ana Prueba"
    refused 1 invalid_follow_up_days settings set --theme dark --follow-up-days 500
    refused 1 import_unreadable prospect import --companies "$scratch/none.csv" --people "$scratch/none.csv"
    refused 2 usage lead nonsense
    refused 2 usage quote create --lead "$lead" --line "sin forma"

    echo "== backup, mutate, restore, compare"
    exported="$(cli export)"
    copy="$(cli backup create | jq -r '.path')"
    [[ -f "$copy" ]] || fail "the backup file does not exist"
    cli lead add --name "Lead Posterior" --phone 987654322 --service REINFO >/dev/null
    cli settings set --add-member "Rosa Prueba" >/dev/null
    expect "the mutation is visible" 4 "$(cli export | jq '.leads | length')"
    restored="$(cli backup restore --file "$copy")"
    [[ -f "$(jq -r '.safety_copy' <<<"$restored")" ]] || fail "the safety copy does not exist"
    expect "restored state" "$exported" "$(cli export)"
    refused 3 storage_error backup restore --file "$scratch/companies.csv"

    echo "== status and read-only safety"
    state="$(cli status)"
    expect "schema version" 8 "$(jq '.schema_version' <<<"$state")"
    expect "integrity check" ok "$(jq -r '.integrity_check' <<<"$state")"
    expect "status leads" 3 "$(jq '.counts.leads' <<<"$state")"
    expect "status cases" 2 "$(jq '.counts.cases' <<<"$state")"
    expect "status active cases" 1 "$(jq '.counts.active_cases' <<<"$state")"
    expect "status quotes" 1 "$(jq '.counts.quotes' <<<"$state")"
    expect "status prospects" 3 "$(jq '.counts.prospects' <<<"$state")"
    expect "export names the schema" 8 "$(cli export | jq '.schema_version')"
    for read in \
        "status" "export" "agenda" "lead list" "lead summary" \
        "lead show $lead" "lead whatsapp $lead" \
        "prospect search --query piura" "prospect count" \
        "case list" "case show $case" "case show $settled" \
        "quote list" "quote show $quote" "quote pdf $quote --out lectura.pdf" \
        "settings show"; do
        # shellcheck disable=SC2086
        read_only $read
    done
    status=0
    "$binary" --db absent.db --json status >/dev/null 2>&1 || status=$?
    expect "a read on a missing database" 3 "$status"
    [[ ! -e absent.db ]] || fail "a read created the database"
    status=0
    env -u COMINORSA_DB_PATH "$binary" --json status >/dev/null 2>&1 || status=$?
    expect "no database path" 2 "$status"
}

run_in() {
    mkdir -p "$1"
    cd "$1"
    scenario
    cd - >/dev/null
}

run_in "$root/first"

echo "== two runs give identical state"
scenario_again="$(run_in "$root/second" 2>&1)" || {
    echo "$scenario_again" >&2
    fail "the second run did not pass"
}
export_of() {
    "$binary" --db "$1/crm.db" --json export
}
expect "byte-identical export" "$(export_of "$root/first")" "$(export_of "$root/second")"
[[ "$(export_of "$root/first" | jq '.data.quotes | length')" == 1 ]] || fail "the export lost its quotes"
expect "export hash" "$(export_of "$root/first" | sha256sum)" "$(export_of "$root/second" | sha256sum)"

echo "== a reused id seed is refused, an identical repeat is not"
cd "$root"
cp "$root/first/crm.db" seeds.db
db=seeds.db
seeded() {
    local seed="$1"
    shift
    "$binary" --db "$db" --json --today "$day" --id-seed "$seed" "$@" | jq -c '.data'
}
signed_case() {
    local id
    id="$(seeded "$1" lead add --name "$2" --phone "$3" --service "$4" --follow-up 2026-04-01 | jq -r '.lead.id')"
    seeded "$1-1" lead advance "$id" >/dev/null
    seeded "$1-2" lead advance "$id" >/dev/null
    seeded "$5" lead advance "$id" | jq -r '.case.id'
}
seed_case="$(signed_case uno Uno 987654341 DAC abre)"
seeded extra billing add "$seed_case" --title Extra --amount 100 --due 2026-05-01 >/dev/null
hash_before="$(state_hash)"
expect "an identical billing repeat changes nothing" false \
    "$(seeded extra billing add "$seed_case" --title Extra --amount 100 --due 2026-05-01 | jq '.changed')"
expect "state after an identical repeat" "$hash_before" "$(state_hash)"
refused 1 id_seed_reused --id-seed extra billing add "$seed_case" --title Otro --amount 999 --due 2026-06-01
seeded zeta lead add --name Zeta --phone 987654342 --service REINFO --follow-up 2026-04-02 >/dev/null
refused 1 id_seed_reused --id-seed zeta case close "$seed_case"
dos="$(seeded dos lead add --name Dos --phone 987654343 --service REINFO --follow-up 2026-04-03 | jq -r '.lead.id')"
seeded dos-1 lead advance "$dos" >/dev/null
seeded dos-2 lead advance "$dos" >/dev/null
refused 1 id_seed_reused --id-seed abre lead advance "$dos"
primera="$(seeded p1 quote create --lead "$dos" --line "a|1|100" | jq -r '.quote.number')"
segunda="$(seeded p2 quote create --lead "$dos" --line "b|1|100" | jq -r '.quote.number')"
seeded p3 quote send "$primera" >/dev/null
seeded p4 quote send "$segunda" >/dev/null
seeded version quote version "$primera" >/dev/null
refused 1 id_seed_reused --id-seed version quote version "$segunda"
db=crm.db

echo "== an explicit option takes effect or fails"
db=seeds.db
refused 1 member_not_found settings set --remove-member "Nadie Existe"
refused 1 owner_not_in_team lead assign "$dos" --owner Fantasma
expect "assigning a member" "Ana Prueba" "$(cli lead assign "$dos" --owner "Ana Prueba" | jq -r '.lead.owner')"
expect "clearing the owner" null "$(cli lead assign "$dos" --owner "" | jq -c '.lead.owner')"
refused 2 usage billing edit "$no_id"
echo "existing" >victim.csv
refused 1 pdf_target_exists quote pdf "$primera" --out victim.csv
expect "a refused PDF keeps the file" existing "$(cat victim.csv)"
refused 1 pdf_target_is_database quote pdf "$primera" --out seeds.db --force
"$binary" --db "$db" --json quote pdf "$primera" --out victim.csv --force >/dev/null
expect "force writes the PDF" "%PDF" "$(head -c 4 victim.csv)"
db=crm.db

echo "== an older schema is reported, never migrated by a read"
cp "$root/first/crm.db" old.db
sqlite3 old.db "PRAGMA journal_mode = DELETE;
    DROP TABLE quote_lines; DROP TABLE quotes; DROP TABLE quote_numbers;
    DELETE FROM _sqlx_migrations WHERE version = 8;" >/dev/null
old_hash="$(file_hash old.db)"
for read in \
    "status" "export" "agenda" "lead list" "prospect count" "case list" \
    "quote list" "settings show"; do
    status=0
    # shellcheck disable=SC2086
    "$binary" --db old.db --json $read >old.out 2>old.err || status=$?
    expect "exit of '$read' on an older schema" 3 "$status"
    expect "stdout of '$read' on an older schema" "" "$(cat old.out)"
    expect "code of '$read' on an older schema" schema_outdated "$(jq -r '.error.code' old.err)"
    expect "file hash after '$read' on an older schema" "$old_hash" "$(file_hash old.db)"
done
message="$(jq -r '.error.message' old.err)"
[[ "$message" == *"7"* && "$message" == *"8"* && "$message" == *"cominorsa migrate"* ]] ||
    fail "the error must name both versions and the upgrade: $message"

refused_writes old.db schema_outdated
expect "a refused write leaves the schema" 7 "$(sqlite3 old.db 'SELECT MAX(version) FROM _sqlx_migrations')"

echo "== a failed migration leaves the database and keeps its copy"
cp old.db blocked.db
sqlite3 blocked.db "CREATE TABLE quote_numbers (blocking TEXT);"
blocked_hash="$(file_hash blocked.db)"
status=0
"$binary" --db blocked.db --json --today "$day" migrate >blocked.out 2>blocked.err || status=$?
expect "exit of a failed migration" 3 "$status"
expect "stdout of a failed migration" "" "$(cat blocked.out)"
expect "code of a failed migration" migration_failed "$(jq -r '.error.code' blocked.err)"
expect "file hash after a failed migration" "$blocked_hash" "$(file_hash blocked.db)"
blocked_copy="$(compgen -G 'blocked.antes-de-migrar-*.db')" || fail "a failed migration lost its copy"
expect "the copy holds the old schema" 7 "$(sqlite3 "$blocked_copy" 'SELECT MAX(version) FROM _sqlx_migrations')"
[[ "$(jq -r '.error.message' blocked.err)" == *"$blocked_copy"* ]] ||
    fail "the error must name the copy: $(cat blocked.err)"
[[ -z "$(compgen -G 'blocked.migrating*')" ]] || fail "a failed migration left its working copy"

echo "== only migrate upgrades"
migration="$("$binary" --db old.db --json --today "$day" migrate | jq -c '.data')"
expect "migrate changed" true "$(jq '.changed' <<<"$migration")"
expect "migrate from" 7 "$(jq '.from' <<<"$migration")"
expect "migrate to" 8 "$(jq '.to' <<<"$migration")"
[[ -f "$(jq -r '.safety_copy' <<<"$migration")" ]] || fail "migrate left no safety copy"
db=old.db
expect "a read works after migrating" 8 "$(cli status | jq '.schema_version')"
expect "migrating twice changes nothing" false "$(cli migrate | jq '.changed')"

echo "== a newer schema is refused"
cp "$root/first/crm.db" new.db
sqlite3 new.db "PRAGMA journal_mode = DELETE;
    INSERT INTO _sqlx_migrations
    SELECT 999, description, installed_on, success, checksum, execution_time
    FROM _sqlx_migrations WHERE version = 8;" >/dev/null
new_hash="$(file_hash new.db)"
status=0
"$binary" --db new.db --json status >/dev/null 2>new.err || status=$?
expect "exit on a newer schema" 3 "$status"
expect "code on a newer schema" schema_newer "$(jq -r '.error.code' new.err)"
expect "file hash on a newer schema" "$new_hash" "$(file_hash new.db)"
refused_writes new.db schema_newer
expect "a refused write keeps the journal mode" delete "$(sqlite3 new.db 'PRAGMA journal_mode')"
cd - >/dev/null

printf '\nCLI end-to-end scenario passed: %d checks.\n' "$checks"
