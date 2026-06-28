#!/usr/bin/env bash
#
# Hypha Protocol — end-to-end integration test against LIVE testnet contracts.
#
# Exercises the full ERC-8004 loop with real signed transactions:
#   1. register a fresh agent in the Identity Registry (owner-authed)
#   2. leave PERMISSIONLESS feedback (no validation prerequisite — proves the decoupling)
#   3. read the reputation summary
#   4. two-phase validation: owner requests -> named validator responds (graded 0..=100)
#   5. read the validation summary
#   6. stake / unstake SEP-41 (native XLM) collateral
#
# Requires: stellar CLI, two funded testnet keys. Contract IDs are read from deployments/testnet.json.
#
# Usage: OWNER_KEY=deployer CLIENT_KEY=temp ./scripts/e2e-testnet.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPLOY="$ROOT/deployments/testnet.json"
NET=testnet
OWNER_KEY="${OWNER_KEY:-deployer}"
CLIENT_KEY="${CLIENT_KEY:-temp}"

read_id() { node -e "console.log(require('$DEPLOY').contracts.$1)"; }
IDENTITY=$(read_id identity)
REPUTATION=$(read_id reputation)
VALIDATION=$(read_id validation)
STAKING=$(read_id staking)

OWNER=$(stellar keys address "$OWNER_KEY")
CLIENT=$(stellar keys address "$CLIENT_KEY")
NATIVE_SAC=$(stellar contract id asset --asset native --network "$NET")

# Unique 32-byte hashes per run so re-runs don't collide on request_hash.
TS=$(date +%s)
REQ_HASH=$(printf '%064x' "$TS")
FB_HASH=$(printf '%064x' "$((TS + 1))")

inv() { local id="$1"; local src="$2"; shift 2; stellar contract invoke --id "$id" --source "$src" --network "$NET" -- "$@" 2>/dev/null; }
# Allow a freshly-submitted write to propagate before a dependent step simulates against it.
settle() { sleep 6; }

echo "▸ identity=$IDENTITY"
echo "▸ owner=$OWNER  client/validator=$CLIENT"
echo

echo "1) register agent (owner-authed)"
AGENT_ID=$(inv "$IDENTITY" "$OWNER_KEY" register --owner "$OWNER" --agent_uri "ipfs://hypha-e2e-$TS" --metadata '[]')
echo "   -> agent_id=$AGENT_ID"
settle
echo "   owner_of=$(inv "$IDENTITY" "$OWNER_KEY" owner_of --agent_id "$AGENT_ID")"
echo

# Note: the stellar CLI's arg parser rejects empty-string values, so this script uses non-empty
# tags throughout. The contracts still treat "" as "match any" when called by other clients/SDKs.
echo "2) give_feedback from a NON-owner client — NO validation record exists yet"
IDX=$(inv "$REPUTATION" "$CLIENT_KEY" give_feedback \
  --client "$CLIENT" --agent_id "$AGENT_ID" --value 5 --value_decimals 0 \
  --tag1 quality --tag2 v1 --endpoint none --feedback_uri none --feedback_hash "$FB_HASH")
echo "   -> feedback index=$IDX (succeeded without any validation — decoupling proven)"
echo

settle
echo "3) reputation summary (count, summaryValue@18dp, decimals)"
echo "   -> $(inv "$REPUTATION" "$CLIENT_KEY" get_summary --agent_id "$AGENT_ID" --clients "[\"$CLIENT\"]" --tag1 quality --tag2 v1)"
echo

echo "4) two-phase validation: owner requests -> validator responds (graded)"
inv "$IDENTITY" "$OWNER_KEY" agent_exists --agent_id "$AGENT_ID" >/dev/null
inv "$VALIDATION" "$OWNER_KEY" validation_request \
  --requester "$OWNER" --agent_id "$AGENT_ID" --validator "$CLIENT" \
  --request_uri "ipfs://req-$TS" --request_hash "$REQ_HASH" >/dev/null
settle
echo "   request recorded; validator responding with score 90"
inv "$VALIDATION" "$CLIENT_KEY" validation_response \
  --validator "$CLIENT" --request_hash "$REQ_HASH" --response 90 \
  --response_uri "ipfs://resp-$TS" --response_hash "$REQ_HASH" --tag stake >/dev/null
echo "   status=$(inv "$VALIDATION" "$OWNER_KEY" get_validation_status --request_hash "$REQ_HASH")"
echo

settle
echo "5) validation summary (count, averageResponse)"
echo "   -> $(inv "$VALIDATION" "$OWNER_KEY" get_summary --agent_id "$AGENT_ID" --validators "[\"$CLIENT\"]" --tag stake)"
echo

echo "6) staking: deposit 1 XLM (native SAC) then unstake"
inv "$STAKING" "$CLIENT_KEY" deposit_stake --agent "$CLIENT" --token "$NATIVE_SAC" --amount 10000000 >/dev/null
settle
echo "   staked=$(inv "$STAKING" "$CLIENT_KEY" get_stake --agent "$CLIENT")"
settle
inv "$STAKING" "$CLIENT_KEY" unstake --agent "$CLIENT" --token "$NATIVE_SAC" --amount 10000000 >/dev/null
echo "   after unstake=$(inv "$STAKING" "$CLIENT_KEY" get_stake --agent "$CLIENT")"
echo
echo "✅ e2e complete — full ERC-8004 loop verified on live testnet."
