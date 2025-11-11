#!/bin/bash
# find_engine.sh (guid / version only) – safe debug logging

# Set -e to exit immediately if a command exits with a non-zero status.
# Set -u to treat unset variables as an error.
set -eu

# Debug logging (FIND_ENGINE_DEBUG=1 in environment to enable)
DBG_LOG_PREFIX="[find_engine.sh]"
debug_log() {
    if [[ "${FIND_ENGINE_DEBUG:-0}" == "1" ]]; then
        echo "$DBG_LOG_PREFIX DEBUG: $*" >&2
    fi
}
error_log() {
    echo "$DBG_LOG_PREFIX ERROR: $*" >&2
}

TYPE="$1"
VAL="$2"

if [[ -z "$TYPE" ]]; then
    error_log "missing TYPE argument"
    exit 1
fi
if [[ -z "$VAL" ]]; then
    error_log "missing VALUE argument"
    exit 1
fi

debug_log "TYPE=$TYPE VAL=$VAL"

ENGINE_PATH=""
OS_NAME=$(uname -s) # e.g., Linux, Darwin

case "$TYPE" in
    guid)
        # GUID resolution for Unreal Engine is primarily a Windows registry feature.
        # On Linux/macOS, Epic Games Launcher typically uses version strings in Install.ini.
        # Custom builds might use GUIDs, but this helper doesn't support that lookup via Install.ini.
        error_log "GUID resolution not directly supported by Install.ini on $OS_NAME for Epic Games Launcher builds."
        exit 1
        ;;
    version)
        debug_log "Attempting to resolve version '$VAL' for OS '$OS_NAME'"
        VERSION_CLEAN=$(echo "$VAL" | tr '.' '_') # e.g., 5.6 -> UE_5_6 in INI files

        if [[ "$OS_NAME" == "Linux" ]]; then
            INI_FILE="$HOME/.config/Epic/UnrealEngine/Install.ini"
            debug_log "Looking for version '$VAL' in Linux Install.ini at '$INI_FILE'"

            # Try to read from Install.ini
            if [[ -f "$INI_FILE" ]]; then
                # --- ▼▼▼ 修正箇所 ▼▼▼ ---
                # 1. カスタムビルドID (例: "UEQ5.6", "My[Build]") をそのまま探す
                # grepで特殊文字として扱われないようエスケープ
                ESCAPED_VAL_CUSTOM=$(echo "$VAL" | sed 's/[]\.^$*\[\\]/\\&/g') # <-- 閉じカッコ]を追加し、]と\もエスケープ
                debug_log "Checking for custom ID match: ^${ESCAPED_VAL_CUSTOM}="
                ENGINE_PATH=$(grep -i "^${ESCAPED_VAL_CUSTOM}=" "$INI_FILE" | head -n 1 | cut -d'=' -f2-)

                # 2. 見つからなければ、標準ID (例: "UE_5.6") を探す
                if [[ -z "$ENGINE_PATH" ]]; then
                    ESCAPED_VAL_STD=$(echo "$VAL" | sed 's/\./\\./g') # 標準はドットのみエスケープで十分
                    debug_log "Checking for standard ID match: ^UE_${ESCAPED_VAL_STD}="
                    ENGINE_PATH=$(grep -i "^UE_${ESCAPED_VAL_STD}=" "$INI_FILE" | head -n 1 | cut -d'=' -f2-)
                fi
                # --- ▲▲▲ 修正ここまで ▲▲▲ ---
            fi

            if [[ -z "$ENGINE_PATH" ]]; then
                debug_log "Version '$VAL' not found in Install.ini or Install.ini missing. Attempting common fallback paths."
                # Common fallback paths for Linux
                for p in \
                    "$HOME/Epic Games/UE_${VERSION_CLEAN}" \
                    "/opt/Epic Games/UE_${VERSION_CLEAN}" \
                    "/usr/local/share/Epic Games/UE_${VERSION_CLEAN}" \
                    "$HOME/UnrealEngine/UE_${VERSION_CLEAN}" \
                    "/usr/local/UnrealEngine/UE_${VERSION_CLEAN}" \
                    ; do
                    if [[ -d "$p/Engine/Binaries" ]]; then
                        ENGINE_PATH="$p"
                        debug_log "Linux Fallback found: $ENGINE_PATH"
                        break
                    fi
                done
            fi

        elif [[ "$OS_NAME" == "Darwin" ]]; then # macOS
            INI_FILE="$HOME/Library/Application Support/Epic/UnrealEngine/Install.ini"
            debug_log "Looking for version '$VAL' in macOS Install.ini at '$INI_FILE'"

            # Try to read from Install.ini
            if [[ -f "$INI_FILE" ]]; then
                # --- ▼▼▼ 修正箇所 ▼▼▼ ---
                # 1. カスタムビルドID (例: "UEQ5.6", "My[Build]") をそのまま探す
                ESCAPED_VAL_CUSTOM=$(echo "$VAL" | sed 's/[]\.^$*\[\\]/\\&/g') # <-- 閉じカッコ]を追加し、]と\もエスケープ
                debug_log "Checking for custom ID match: ^${ESCAPED_VAL_CUSTOM}="
                ENGINE_PATH=$(grep -i "^${ESCAPED_VAL_CUSTOM}=" "$INI_FILE" | head -n 1 | cut -d'=' -f2-)

                # 2. 見つからなければ、標準ID (例: "UE_5.6") を探す
                if [[ -z "$ENGINE_PATH" ]]; then
                    ESCAPED_VAL_STD=$(echo "$VAL" | sed 's/\./\\./g') # 標準はドットのみエスケープで十分
                    debug_log "Checking for standard ID match: ^UE_${ESCAPED_VAL_STD}="
                    ENGINE_PATH=$(grep -i "^UE_${ESCAPED_VAL_STD}=" "$INI_FILE" | head -n 1 | cut -d'=' -f2-)
                fi
                # --- ▲▲▲ 修正ここまで ▲▲▲ ---
            fi

            if [[ -z "$ENGINE_PATH" ]]; then
                debug_log "Version '$VAL' not found in Install.ini or Install.ini missing. Attempting common fallback paths."
                # Common fallback paths for macOS
                for p in \
                    "/Users/Shared/Epic Games/UE_${VERSION_CLEAN}" \
                    "$HOME/Epic Games/UE_${VERSION_CLEAN}" \
                    "$HOME/Documents/Epic Games/UE_${VERSION_CLEAN}" \
                    ; do
                    if [[ -d "$p/Engine/Binaries" ]]; then
                        ENGINE_PATH="$p"
                        debug_log "macOS Fallback found: $ENGINE_PATH"
                        break
                    fi
                done
            fi

        else
            error_log "Unsupported operating system: $OS_NAME"
            exit 1
        fi

        if [[ -z "$ENGINE_PATH" ]]; then
            error_log "Engine path for version '$VAL' not found via Install.ini or common fallbacks on $OS_NAME."
            exit 1
        fi
        ;;
    path)
        # Lua side handles absolute path resolution and validation.
        error_log "path mode not supported in this helper (should be handled by Lua)"
        exit 1
        ;;
    *)
        error_log "unknown TYPE=$TYPE"
        exit 1
        ;;
esac

# Normalize path (remove trailing slashes)
# 'readlink -f' is more robust for resolving symlinks and normalizing, but not universally available (e.g., on older macOS)
# Using sed for cross-platform compatibility for simple trailing slash removal.
ENGINE_PATH=$(echo "$ENGINE_PATH" | sed 's:/*$::')

debug_log "Resolved ENGINE_PATH (raw): $ENGINE_PATH"

# Validate structure (check for Engine/Binaries)
if [[ ! -d "$ENGINE_PATH/Engine/Binaries" ]]; then
    error_log "Invalid engine structure: '$ENGINE_PATH' (missing Engine/Binaries)"
    exit 1
fi

echo "$ENGINE_PATH"
exit 0
