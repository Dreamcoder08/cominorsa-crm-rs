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

cargo build --quiet --locked -p cominorsa-cli -p cominorsa-importer
binary="$PWD/target/debug/cominorsa"
importer="$PWD/target/debug/cominorsa-import"

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

# importer_refused <file> <args...>: `cominorsa-import` refuses the database like
# `cominorsa` does, with the same exit code and message, and the file keeps its
# hash. The CLI import runs on the same files and is the reference.
importer_refused() {
    local file="$1"
    shift
    local status=0 expected="" before=""
    [[ -e "$file" ]] && before="$(file_hash "$file")"
    "$importer" --db "$file" "$@" >importer.out 2>importer.err || status=$?
    "$binary" --db "$file" status >/dev/null 2>expected.err || expected=$?
    expect "importer exit on $file" "$expected" "$status"
    expect "importer stdout on $file" "" "$(cat importer.out)"
    expect "importer message on $file" "$(cat expected.err)" "$(cat importer.err)"
    if [[ -n "$before" ]]; then
        expect "file hash after the importer on $file" "$before" "$(file_hash "$file")"
    else
        [[ ! -e "$file" ]] || fail "the importer created $file"
    fi
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
    expect "init schema" 9 "$(cli status | jq '.schema_version')"
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
    expect "lead list repeats the follow-up of lead show" \
        "$(cli lead show "$lead" | jq -c '.follow_up')" \
        "$(cli lead list | jq -c '.leads[0].follow_up')"
    expect "a converted prospect has origin prospect" prospect "$(cli lead show "$lead" | jq -r '.lead.origin')"
    expect "a converted prospect starts without consent" null "$(cli lead show "$lead" | jq -c '.lead.consent')"
    refused 1 no_consent lead whatsapp "$lead"
    consent="$(cli lead consent "$lead" --source "Aceptó por llamada" | jq -cS '.lead.consent')"
    expect "recorded consent" '{"given":true,"recorded_on":"'"$day"'","source":"Aceptó por llamada"}' "$consent"
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
    expect "a renewal is an existing client" existing_client "$(jq -r '.renewal.origin' <<<"$closed")"
    expect "a renewal inherits consent" "$consent" "$(jq -cS '.renewal.consent' <<<"$closed")"
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
    expect "schema version" 9 "$(jq '.schema_version' <<<"$state")"
    expect "integrity check" ok "$(jq -r '.integrity_check' <<<"$state")"
    expect "status leads" 3 "$(jq '.counts.leads' <<<"$state")"
    expect "status cases" 2 "$(jq '.counts.cases' <<<"$state")"
    expect "status active cases" 1 "$(jq '.counts.active_cases' <<<"$state")"
    expect "status quotes" 1 "$(jq '.counts.quotes' <<<"$state")"
    expect "status prospects" 3 "$(jq '.counts.prospects' <<<"$state")"
    expect "export names the schema" 9 "$(cli export | jq '.schema_version')"
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
ln seeds.db hard-link.pdf
database_hash="$(file_hash seeds.db)"
refused 1 pdf_target_is_database quote pdf "$primera" --out hard-link.pdf --force
expect "file hash after a PDF on a hard link" "$database_hash" "$(file_hash seeds.db)"
"$binary" --db "$db" --json quote pdf "$primera" --out victim.csv --force >/dev/null
expect "force writes the PDF" "%PDF" "$(head -c 4 victim.csv)"
db=crm.db

echo "== what the app shows is reachable from here"
db=seeds.db
preview="$(cli quote preview --line "a|2|100" --line "b|1|50.50")"
expect "preview total" 29559 "$(jq '.total_cents' <<<"$preview")"
expect "stored template preview" true "$(cli settings whatsapp-preview | jq '.message | length > 0')"
expect "tried template preview" "Sobre REINFO" "$(cli settings whatsapp-preview --template 'Sobre {servicio}' | jq -r '.message')"
expect "follow-up status" upcoming "$(cli lead show "$dos" | jq -r '.follow_up.status')"
expect "a quote change carries its notice" "$segunda marcada como rechazada." \
    "$(cli quote reject "$segunda" | jq -r '.notice')"
db=crm.db

echo "== consent decides the prepared chat"
db=seeds.db
inbound="$(cli lead add --name Entrante --phone 987654351 --service DAC --origin web_form | jq -c '.lead')"
expect "inbound origin" web_form "$(jq -r '.origin' <<<"$inbound")"
expect "inbound consent" '{"given":true,"recorded_on":"'"$day"'","source":"El cliente inició el contacto"}' \
    "$(jq -cS '.consent' <<<"$inbound")"
url="$(cli lead whatsapp "$(jq -r '.id' <<<"$inbound")" | jq -r '.url')"
expect "an inbound lead opens the chat" "https://wa.me/51987654351" "${url%%\?*}"
for origin in referral event existing_client prospect other; do
    added="$(cli lead add --name "Origen $origin" --phone 987654352 --service DAC --origin "$origin" | jq -c '.lead')"
    expect "origin $origin" "$origin" "$(jq -r '.origin' <<<"$added")"
    expect "no consent from $origin" null "$(jq -c '.consent' <<<"$added")"
done
referred="$(cli lead add --name Referida --phone 987654353 --service DAC --origin referral | jq -r '.lead.id')"
expect "no origin means other" other "$(cli lead add --name Simple --phone 987654354 --service DAC | jq -r '.lead.origin')"
refused 1 no_consent lead whatsapp "$referred"
refused 1 invalid_origin lead add --name X --phone 987654355 --service DAC --origin email
refused 1 missing_field lead consent "$referred" --source " "
refused 1 no_consent lead revoke-consent "$referred" --source "Sin consentimiento previo"
refused 1 lead_not_found lead consent "$no_id" --source "Aceptó"
cli lead consent "$referred" --source "Aceptó en persona" >/dev/null
expect "recorded consent opens the chat" true "$(cli lead whatsapp "$referred" | jq '.url | startswith("https://wa.me/51987654353")')"
revoked="$(cli lead revoke-consent "$referred" --source "Pidió no recibir mensajes" | jq -cS '.lead.consent')"
expect "revoked consent" '{"given":false,"recorded_on":"'"$day"'","source":"Pidió no recibir mensajes"}' "$revoked"
refused 1 no_consent lead whatsapp "$referred"
expect "export keeps the revocation" "$revoked" \
    "$(cli export | jq -cS --arg id "$referred" '.leads[] | select(.id == $id) | .consent')"
db=crm.db

echo "== automatic backups copy when due and prune only their own files"
db=auto.db
"$binary" --db "$db" --json init >/dev/null
now=1772470800
copies="$PWD/auto-copias"
# auto_at <unix seconds> [args...]: backup auto at that instant of the fixed day.
auto_at() {
    local at="$1"
    shift
    "$binary" --db "$db" --json --today "$day" --now "$at" backup auto "$@" | jq -c '.data'
}
# auto_on <day> [args...]: backup auto at noon of that day.
auto_on() {
    local on="$1"
    shift
    "$binary" --db "$db" --json --today "$on" backup auto "$@" | jq -c '.data'
}
# Every entry of a folder with a short hash, so any write or delete shows.
listing() {
    (cd "${1:-$copies}" && find . -mindepth 1 | sort | while read -r entry; do
        if [[ -f "$entry" ]]; then
            echo "$entry $(sha256sum <"$entry" | cut -c1-16)"
        else
            echo "$entry dir"
        fi
    done)
}
# auto_refused <exit> <code> <folder> [args...]: backup auto fails with that
# code, and the folder, the database file, and the export stay identical.
auto_refused() {
    local exit_code="$1" code="$2" folder="$3" status=0 files hash state
    shift 3
    files="$(listing "$folder")"
    hash="$(file_hash "$db")"
    state="$(state_hash)"
    "$binary" --db "$db" --json --today "$day" "$@" >auto.out 2>auto.err || status=$?
    expect "exit of '$*' with $code" "$exit_code" "$status"
    expect "code of '$*'" "$code" "$(jq -r '.error.code' auto.err)"
    expect "stdout of '$*'" "" "$(cat auto.out)"
    expect "folder after '$*'" "$files" "$(listing "$folder")"
    expect "file hash after '$*'" "$hash" "$(file_hash "$db")"
    expect "state after '$*'" "$state" "$(state_hash)"
}
expect "no folder means no copy" not_configured "$(auto_at "$now" | jq -r '.status')"
expect "settings default interval" 1 "$(cli settings show | jq '.backup_interval_days')"
expect "settings default kept copies" 14 "$(cli settings show | jq '.backup_keep')"
refused 1 invalid_backup_interval settings set --backup-interval-days 0
refused 1 invalid_backup_interval settings set --backup-interval-days 31
refused 1 invalid_backup_keep settings set --backup-keep 0
refused 1 invalid_backup_keep settings set --backup-keep 91
cli settings set --backup-directory "$PWD/carpeta-ausente" >/dev/null
refused 3 backup_folder_unavailable backup auto
refused 3 backup_folder_unavailable backup auto --dry-run
[[ ! -e carpeta-ausente ]] || fail "a missing backup folder was created"
mkdir -p auto-copias/crm-20240101-090000.db
for name in crm-20260101-090000.db crm-20260201-090000.db notas.txt crm-viejo.db \
    crm-20250101-090000.db.bak otro-20250101-090000.db crm-2025010-090000.db \
    crm-20250195-000095.db crm-20990101-000000.db; do
    echo "$name" >"auto-copias/$name"
done
# A relative folder is refused from anywhere, never resolved where the app runs.
mkdir -p elsewhere
cp -r auto-copias elsewhere/auto-copias
cli settings set --backup-directory auto-copias --backup-keep 2 >/dev/null
absolute_db="$PWD/$db"
(cd elsewhere && db="$absolute_db" && auto_refused 1 backup_folder_not_absolute "$PWD/auto-copias" backup auto)
(cd elsewhere && db="$absolute_db" && auto_refused 1 backup_folder_not_absolute "$PWD/auto-copias" backup auto --dry-run)
auto_refused 1 backup_folder_not_absolute "$copies" backup auto
rm -rf elsewhere
cli settings set --backup-directory "$copies" --backup-keep 2 --backup-interval-days 1 >/dev/null
expect "settings keep the interval" 1 "$(cli export | jq '.settings.backup_interval_days')"
expect "settings keep the count" 2 "$(cli export | jq '.settings.backup_keep')"
files_before="$(listing)"
database_hash="$(file_hash "$db")"
dry="$(auto_at "$now" --dry-run)"
expect "dry run status" would_create "$(jq -r '.status' <<<"$dry")"
expect "dry run copy" "$copies/crm-20260302-120000.db" "$(jq -r '.path' <<<"$dry")"
expect "dry run pruning" "[\"$copies/crm-20260101-090000.db\"]" "$(jq -c '.pruned' <<<"$dry")"
expect "a dry run writes no file" "$files_before" "$(listing)"
expect "a dry run leaves the database" "$database_hash" "$(file_hash "$db")"
created="$(auto_at "$now")"
expect "a due copy is created" created "$(jq -r '.status' <<<"$created")"
[[ -f auto-copias/crm-20260302-120000.db ]] || fail "the automatic copy does not exist"
expect "the dry run predicted the copy" "$(jq -c '{path, pruned}' <<<"$dry")" "$(jq -c '{path, pruned}' <<<"$created")"
expect "the oldest own copy is pruned" "[\"$copies/crm-20260101-090000.db\"]" "$(jq -c '.pruned' <<<"$created")"
expect "unrelated files, impossible and future stamps stay byte-identical" \
    "$(grep -v 'crm-20260101-090000.db ' <<<"$files_before")" \
    "$(listing | grep -v 'crm-20260302-120000.db ')"
expect "the copy records the time" "$now" "$(cli export | jq '.settings.last_backup_at')"
expect "the next copy is due at midnight in Peru" $((now + 43200)) "$(jq '.next_due_at' <<<"$created")"
files_before="$(listing)"
expect "no copy later the same day" not_due "$(auto_at $((now + 39600)) | jq -r '.status')"
expect "a skipped copy writes nothing" "$files_before" "$(listing)"
later="$(auto_on 2026-03-03)"
expect "a daily opening copies the next calendar day" created "$(jq -r '.status' <<<"$later")"
expect "pruning keeps the newest two" "[\"$copies/crm-20260201-090000.db\"]" "$(jq -c '.pruned' <<<"$later")"
expect "own copies left" 2 "$(find auto-copias -maxdepth 1 -type f -name 'crm-2026????-??????.db' | wc -l)"
[[ -f auto-copias/crm-20990101-000000.db ]] || fail "a copy stamped in the future was deleted"
files_before="$(listing)"
state_before="$(state_hash)"
chmod 555 auto-copias
auto_refused 3 backup_folder_unavailable "$copies" --now $((now + 3 * 86400)) backup auto
auto_refused 3 backup_folder_unavailable "$copies" --now $((now + 3 * 86400)) backup auto --dry-run
status=0
"$binary" --db "$db" --json --today "$day" backup create >auto.out 2>auto.err || status=$?
chmod 755 auto-copias
expect "backup create keeps its own failure" "3 storage_error" "$status $(jq -r '.error.code' auto.err)"
expect "an unwritable folder deletes nothing" "$files_before" "$(listing)"
expect "state after an unwritable folder" "$state_before" "$(state_hash)"

echo "== automatic backups never delete the live database or a link to it"
live="$PWD/vivas"
mkdir -p "$live"
db="$live/crm-20200101-000000.db"
"$binary" --db "$db" --json init >/dev/null
cli settings set --backup-directory "$live" --backup-keep 1 >/dev/null
dry="$(auto_at "$now" --dry-run)"
created="$(auto_at "$now")"
expect "the live database is not pruned" '[]' "$(jq -c '.pruned' <<<"$created")"
expect "the dry run agrees on the live database" '[]' "$(jq -c '.pruned' <<<"$dry")"
[[ -f "$db" ]] || fail "the live database was deleted"
expect "the live database still answers" "$now" "$(cli export | jq '.settings.last_backup_at')"
db=linked.db
"$binary" --db "$db" --json init >/dev/null
linked="$PWD/enlazadas"
mkdir -p "$linked"
echo vieja >"$linked/crm-20200101-000000.db"
cli settings set --backup-directory "$linked" --backup-keep 1 >/dev/null
ln "$db" "$linked/crm-20200102-000000.db"
created="$(auto_at "$now")"
expect "a hard link to the live database is not pruned" "[\"$linked/crm-20200101-000000.db\"]" "$(jq -c '.pruned' <<<"$created")"
expect "the link still reaches the live database" "$(file_hash "$db")" "$(file_hash "$linked/crm-20200102-000000.db")"
db=crm.db

echo "== an older schema is reported, never migrated by a read"
cp "$root/first/crm.db" old.db
sqlite3 old.db "PRAGMA journal_mode = DELETE;
    DROP TABLE quote_lines; DROP TABLE quotes; DROP TABLE quote_numbers;
    ALTER TABLE leads DROP COLUMN origin; ALTER TABLE leads DROP COLUMN consent_given;
    ALTER TABLE leads DROP COLUMN consent_on; ALTER TABLE leads DROP COLUMN consent_source;
    DELETE FROM _sqlx_migrations WHERE version IN (8, 9);" >/dev/null
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
[[ "$message" == *"7"* && "$message" == *"9"* && "$message" == *"cominorsa migrate"* ]] ||
    fail "the error must name both versions and the upgrade: $message"

refused_writes old.db schema_outdated
expect "a refused write leaves the schema" 7 "$(sqlite3 old.db 'SELECT MAX(version) FROM _sqlx_migrations')"

echo "== the importer follows the same contract"
cat >import-companies.csv <<CSV
titular,perfil_icp,n_concesiones,hectareas_totales,departamentos,provincias,sustancias,estados_concesion,servicio_potencial,revisar_manual,fuente_dato
Empresa Importada SAC,PERFIL_1_formalizacion,1,10,PIURA,SULLANA,M,TITULADA,DAC,FALSE,Synthetic
CSV
cat >import-tax.csv <<CSV
titular,match,ruc,razon_social,estado,condicion,ubigeo,domicilio
Empresa Importada SAC,unico,20123456786,EMPRESA IMPORTADA SAC,ACTIVO,HABIDO,200101,CAL UNO
CSV
for file in absent.db old.db; do
    importer_refused "$file" import-companies.csv import-companies.csv
    importer_refused "$file" --sunat import-tax.csv
done
status=0
"$importer" --db absent.db import-companies.csv none.csv >importer.out 2>importer.err || status=$?
expect "invalid input wins over the database" 1 "$status"
[[ ! -e absent.db ]] || fail "invalid input created the database"
"$binary" --db imported.db --json init >/dev/null
sqlite3 imported.db "PRAGMA journal_mode = DELETE;" >/dev/null
imported_hash="$(file_hash imported.db)"
status=0
"$importer" --db imported.db import-companies.csv none.csv >importer.out 2>importer.err || status=$?
expect "invalid input on a current database" 1 "$status"
expect "file hash after invalid input" "$imported_hash" "$(file_hash imported.db)"
status=0
"$importer" --db imported.db --sunat none.csv >importer.out 2>importer.err || status=$?
expect "invalid tax file on a current database" 1 "$status"
expect "file hash after an invalid tax file" "$imported_hash" "$(file_hash imported.db)"
sed 's/Empresa Importada SAC/Persona Importada/' import-companies.csv >import-people.csv
imported="$("$importer" --db imported.db import-companies.csv import-people.csv)"
expect "the importer reports what it added" "Prospectos nuevos: 2" "$(sed -n 1p <<<"$imported")"
expect "the importer reports the total" "Prospectos totales: 2" "$(sed -n 2p <<<"$imported")"
[[ -f "$(sed -n 3p <<<"$imported" | sed 's/^Copia previa: //')" ]] || fail "the importer lost its copy"
expect "the import reached the database" 2 "$("$binary" --db imported.db --json prospect count | jq '.data.total')"
enriched="$("$importer" --db imported.db --sunat import-tax.csv)"
expect "the importer reports what it enriched" "Empresas actualizadas: 1" "$(sed -n 1p <<<"$enriched")"
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
expect "migrate to" 9 "$(jq '.to' <<<"$migration")"
[[ -f "$(jq -r '.safety_copy' <<<"$migration")" ]] || fail "migrate left no safety copy"
db=old.db
expect "a read works after migrating" 9 "$(cli status | jq '.schema_version')"
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
importer_refused new.db import-companies.csv import-people.csv
importer_refused new.db --sunat import-tax.csv
expect "a refused write keeps the journal mode" delete "$(sqlite3 new.db 'PRAGMA journal_mode')"
cd - >/dev/null

printf '\nCLI end-to-end scenario passed: %d checks.\n' "$checks"
