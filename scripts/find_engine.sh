#!/usr/bin/env bash
# ======================================================================
# find_engine.sh (guid / version only)
#
# Usage:
#   ./find_engine.sh version 5.6
#   ./find_engine.sh guid {GUID}
#
# STDOUT: 成功時エンジンルートのみ
# STDERR: エラー/デバッグ (FIND_ENGINE_DEBUG=1)
#
# Exit codes:
#   0 = success
#   1 = bad args / unknown type
#   2 = not found
#   3 = invalid structure (missing Engine/Binaries)
# ======================================================================

set -u

DBG="${FIND_ENGINE_DEBUG:-}"
log() {
  [ "$DBG" = "1" ] && printf '[find_engine] %s\n' "$*" 1>&2
}

err() {
  printf '[find_engine] ERROR: %s\n' "$*" 1>&2
}

TYPE="${1:-}"
VALUE="${2:-}"

if [ -z "$TYPE" ]; then
  err "missing TYPE"
  exit 1
fi
if [ -z "$VALUE" ]; then
  err "missing VALUE"
  exit 1
fi

# Allow TYPE=VALUE form
case "$TYPE" in
  *=*)
    lhs="${TYPE%%=*}"
    rhs="${TYPE#*=}"
    if [ -z "$VALUE" ]; then
      TYPE="$lhs"
      VALUE="$rhs"
    fi
    ;;
esac

case "$TYPE" in
  guid|GUID|Guid) TYPE="guid" ;;
  version|Version|VERSION) TYPE="version" ;;
  path|PATH)
    err "path mode not supported in helper (handle absolute paths in Lua)"
    exit 1
    ;;
esac

log "TYPE=$TYPE VALUE=$VALUE"

ENGINEPATH=""

# --- Helpers ------------------------------------------------------------

validate_engine_root() {
  local p="$1"
  [ -d "$p/Engine" ] || return 1
  # Mac/Lnx: Binaries may contain platform subdirs, allow either:
  if [ -d "$p/Engine/Binaries" ] || [ -d "$p/Engine/Build" ]; then
    return 0
  fi
  return 1
}

resolve_guid() {
  local guid="$1"
  # Ensure braces
  if [[ "$guid" != \{*} ]]; then
    guid="{$guid}"
  fi

  # Potential locations of Installations mapping file
  # (Some variations included for robustness)
  local files=(
    "$HOME/Library/Application Support/Epic/UnrealEngine/Installations"
    "$HOME/Library/Application Support/Epic/Unreal Engine/Installations"
    "$HOME/Library/Application Support/Epic/UE_Installations" # rare
    "$XDG_CONFIG_HOME/Epic/UnrealEngine/Installations"
    "$HOME/.config/Epic/UnrealEngine/Installations"
  )

  for f in "${files[@]}"; do
    [ -f "$f" ] || continue
    log "scan file: $f"
    # Read line by line: {GUID}=/path/to/engine
    # Avoid IFS splitting inside path: use while read -r whole
    while IFS= read -r line || [ -n "$line" ]; do
      # Trim
      line="${line#"${line%%[![:space:]]*}"}"
      line="${line%"${line##*[![:space:]]}"}"
      [ -z "$line" ] && continue
      case "$line" in
        \#*) continue ;;
      esac
      # Split first '='
      key="${line%%=*}"
      val="${line#*=}"
      key="${key%"${key##*[![:space:]]}"}"
      key="${key#"${key%%[![:space:]]*}"}"
      val="${val#"${val%%[![:space:]]*}"}"
      val="${val%"${val##*[![:space:]]}"}"
      if [ "$key" = "$guid" ]; then
        log "GUID match in $f: $val"
        ENGINEPATH="$val"
        return 0
      fi
    done <"$f"
  done
  return 1
}

resolve_version() {
  local ver="$1"
  # Candidate directories (add or reorder as needed)
  local candidates=(
    "/Users/Shared/Epic Games/UE_$ver"
    "/Users/Shared/EpicGames/UE_$ver"
    "/Applications/Epic Games/UE_$ver"
    "/Applications/EpicGames/UE_$ver"
    "/Users/Shared/UnrealEngine/UE_$ver"
    "$HOME/UnrealEngine/UE_$ver"
    "$HOME/Epic/UE_$ver"
    "/opt/Epic/UE_$ver"
    "/opt/UnrealEngine/UE_$ver"
  )
  for p in "${candidates[@]}"; do
    if [ -d "$p/Engine/Binaries" ]; then
      log "version fallback hit: $p"
      ENGINEPATH="$p"
      return 0
    fi
  done
  return 1
}

# --- Dispatch -----------------------------------------------------------

case "$TYPE" in
  guid)
    if resolve_guid "$VALUE"; then
      :
    else
      err "engine path not found for GUID"
      exit 2
    fi
    ;;
  version)
    if resolve_version "$VALUE"; then
      :
    else
      err "engine path not found for version $VALUE"
      exit 2
    fi
    ;;
  *)
    err "unknown TYPE: $TYPE"
    exit 1
    ;;
esac

# Normalize path (remove trailing slashes)
# Use parameter expansion repeatedly
while [ "${ENGINEPATH%/}" != "$ENGINEPATH" ]; do
  ENGINEPATH="${ENGINEPATH%/}"
done

if ! validate_engine_root "$ENGINEPATH"; then
  err "invalid engine structure: $ENGINEPATH"
  exit 3
fi

# Success
printf '%s\n' "$ENGINEPATH"
exit 0
