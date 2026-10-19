#!/usr/bin/env bash
# This code was written by: Hikari.
# Non-invasive live accelerator manager for iNiR (never modifies /home/user/inir or /home/user/inir-cli).
set -euo pipefail

PLUGIN_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="/mnt/build/inir-rust-plugin-target"
RELEASE_DIR="${TARGET_DIR}/release"
BUILT_BIN="${RELEASE_DIR}/inir-accel"

SHIM_DIR="${PLUGIN_ROOT}/bin"
INSTALLED_BIN="${SHIM_DIR}/inir-accel"

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/quickshell"
STATE_GEN_DIR="${STATE_DIR}/user/generated"
STATE_BIN_DIR="${STATE_GEN_DIR}/bin"
VENV_BIN_DIR="${STATE_DIR}/.venv/bin"

usage() {
    cat <<EOF
Usage: $(basename "$0") [install|uninstall|status|env]

Commands:
  install    Copy release binary into ${INSTALLED_BIN} and activate live Rust hooks in
             ${STATE_BIN_DIR}/inir-washi and ${VENV_BIN_DIR}/python3
             (Zero modifications to /home/user/inir; survives target/ cache cleanups)
  uninstall  Remove hooks and restore original venv python3 and Go inir-washi
  status     Show active accelerator hooks and build status
  env        Print shell export lines for PATH and QML2_IMPORT_PATH
EOF
}

cmd="${1:-status}"

# Only what test/parity.py proved byte for byte the same as the original takes over a live hook. The other Go
# helpers keep (or get back) their original binary; the shell rebuilds a missing one on demand.
PARITY_GO=(inir-washi inir-opencode-themegen)
ALL_GO=(inir-washi inir-vscode-themegen inir-zed-themegen inir-opencode-themegen inir-system24-themegen)

restore_go() {
    if [[ -L "${STATE_BIN_DIR}/$1" ]]; then
        rm -f "${STATE_BIN_DIR}/$1"
    fi
    if [[ -f "${STATE_BIN_DIR}/$1.go.bak" ]]; then
        mv -f "${STATE_BIN_DIR}/$1.go.bak" "${STATE_BIN_DIR}/$1"
    fi
}

case "$cmd" in
    install)
        if [[ ! -x "$BUILT_BIN" && ! -x "$INSTALLED_BIN" ]]; then
            echo "[inir-rust-plugin] Building release binaries first..."
            cargo build --release --manifest-path "${PLUGIN_ROOT}/Cargo.toml"
        fi
        mkdir -p "$SHIM_DIR" "$STATE_BIN_DIR"

        if [[ -x "$BUILT_BIN" ]]; then
            cp -f "$BUILT_BIN" "${INSTALLED_BIN}.tmp"
            chmod +x "${INSTALLED_BIN}.tmp"
            mv -f "${INSTALLED_BIN}.tmp" "$INSTALLED_BIN"
        fi

        # 1. Create multi-call symlinks in ${PLUGIN_ROOT}/bin
        for sym in \
            inir-washi \
            inir-vscode-themegen \
            inir-zed-themegen \
            inir-opencode-themegen \
            inir-system24-themegen \
            generate_colors_material.py \
            generate_terminal_configs.py \
            claude_code_theme.py \
            system24_palette.py \
            extract_cover_colors.py \
            resolve_audio_source.py \
            clipboard-store.py \
            keyboard_lock_state_daemon.py \
            dev_hot_reload.py \
            least_busy_region.py \
            thumbgen.py \
            detect_sensors.py \
            scan-plugins.py \
            scan-widgets.sh \
            icon-theme-for-mode.sh
        do
            ln -sf "$INSTALLED_BIN" "${SHIM_DIR}/${sym}"
        done

        mkdir -p "${HOME}/.local/bin"
        ln -sf "$INSTALLED_BIN" "${HOME}/.local/bin/inir-accel"

        # 2. Hook into iNiR's native state bin cache ($STATE_BIN_DIR/inir-*)
        for gobin in "${ALL_GO[@]}"; do
            [[ " ${PARITY_GO[*]} " == *" ${gobin} "* ]] || { restore_go "$gobin"; continue; }
            if [[ -f "${STATE_BIN_DIR}/${gobin}" && ! -L "${STATE_BIN_DIR}/${gobin}" ]]; then
                mv -f "${STATE_BIN_DIR}/${gobin}" "${STATE_BIN_DIR}/${gobin}.go.bak"
            fi
            ln -sf "$INSTALLED_BIN" "${STATE_BIN_DIR}/${gobin}"
        done

        # 3. Hook into iNiR's runtime virtualenv (${VENV_BIN_DIR}/python3)
        if [[ -d "$VENV_BIN_DIR" ]]; then
            if [[ -e "${VENV_BIN_DIR}/python3" && ! -e "${VENV_BIN_DIR}/python3.real" ]]; then
                REAL_PY="$(readlink -f "${VENV_BIN_DIR}/python3")"
                ln -sf "$REAL_PY" "${VENV_BIN_DIR}/python3.real"
            fi
            ln -sf "$INSTALLED_BIN" "${VENV_BIN_DIR}/python3"
        fi

        echo "[inir-rust-plugin] Live Rust accelerators installed:"
        echo "  - Resident bin : ${INSTALLED_BIN} (linked at ~/.local/bin/inir-accel)"
        echo "  - Go hooks     : ${PARITY_GO[*]} -> ${INSTALLED_BIN} (the rest stay original)"
        echo "  - Venv python3 : ${VENV_BIN_DIR}/python3 -> ${INSTALLED_BIN} (Material and region; the rest go to python3.real)"
        ;;
    uninstall)
        for gobin in "${ALL_GO[@]}"; do
            restore_go "$gobin"
        done
        if [[ -e "${VENV_BIN_DIR}/python3.real" ]]; then
            REAL_PY="$(readlink -f "${VENV_BIN_DIR}/python3.real")"
            ln -sf "$REAL_PY" "${VENV_BIN_DIR}/python3"
            rm -f "${VENV_BIN_DIR}/python3.real"
        fi
        echo "[inir-rust-plugin] Uninstalled shims and restored original venv python3 and Go binaries."
        ;;
    env)
        QML_IMPORT_DIR="$(find "${RELEASE_DIR}/build" -type d -name "qml_modules" 2>/dev/null | head -n1 || true)"
        echo "export PATH=\"${SHIM_DIR}:\$PATH\""
        if [[ -n "$QML_IMPORT_DIR" ]]; then
            echo "export QML2_IMPORT_PATH=\"${QML_IMPORT_DIR}:\${QML2_IMPORT_PATH:-}\""
        fi
        ;;
    status|*)
        echo "=== inir-rust-plugin Status ==="
        echo "Workspace      : ${PLUGIN_ROOT}"
        echo "Resident Binary: ${INSTALLED_BIN} ($( [[ -x "$INSTALLED_BIN" ]] && echo "INSTALLED & ACTIVE" || echo "NOT INSTALLED" ))"
        for gobin in "${ALL_GO[@]}"; do
            echo "Go Hook (${gobin}): $(readlink -f "${STATE_BIN_DIR}/${gobin}" 2>/dev/null || echo "none")"
        done
        echo "Venv Hook      : ${VENV_BIN_DIR}/python3 -> $(readlink -f "${VENV_BIN_DIR}/python3" 2>/dev/null || echo "none")"
        ;;
esac
