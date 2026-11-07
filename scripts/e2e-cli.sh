#!/usr/bin/env bash
# End-to-end proof through the `cominorsa` CLI alone, on synthetic data in a
# fresh temporary database. Usage: scripts/e2e-cli.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# A missing assertion tool must fail the gate, never skip it.
for tool in jq sha256sum; do
    if ! command -v "$tool" >/dev/null; then
        echo "FAILED: $tool is required to assert the scenario" >&2
        exit 1
    fi
done

cargo build --quiet --locked -p cominorsa-cli
binary="$PWD/target/debug/cominorsa"

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
db="$scratch/crm.db"

day="2026-03-02"
checks=0

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

echo "== team and settings"
cli settings set \
    --company-name "Consultora Sintética SAC" --company-ruc 20100070970 \
    --add-member "Ana Prueba" --add-member "Luis Prueba" \
    --follow-up-days 7 --igv 18 --backup-directory "$scratch/copias" >/dev/null
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
quote="$(cli quote create --lead "$lead" \
    --line "Estudio de coordenadas|2|500.00" --line "Trámite DAC|1|1000.00" --notes "Sintética" |
    jq -r '.quote.number')"
expect "quote number" "COT-2026-0001" "$quote"
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
expect "the mutation is visible" 3 "$(cli export | jq '.leads | length')"
restored="$(cli backup restore --file "$copy")"
[[ -f "$(jq -r '.safety_copy' <<<"$restored")" ]] || fail "the safety copy does not exist"
expect "restored state" "$exported" "$(cli export)"
refused 3 storage_error backup restore --file "$scratch/companies.csv"

echo "== status and read-only safety"
counts="$(cli status | jq -c '.counts')"
expect "status leads" 2 "$(jq '.leads' <<<"$counts")"
expect "status cases" 1 "$(jq '.active_cases' <<<"$counts")"
expect "status quotes" 1 "$(jq '.quotes' <<<"$counts")"
expect "status prospects" 3 "$(jq '.prospects' <<<"$counts")"
status=0
"$binary" --db "$scratch/absent.db" --json status >/dev/null 2>&1 || status=$?
expect "a read on a missing database" 3 "$status"
[[ ! -e "$scratch/absent.db" ]] || fail "a read created the database"
status=0
env -u COMINORSA_DB_PATH "$binary" --json status >/dev/null 2>&1 || status=$?
expect "no database path" 2 "$status"

printf '\nCLI end-to-end scenario passed: %d checks.\n' "$checks"
