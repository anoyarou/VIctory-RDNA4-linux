#!/usr/bin/env bash
# Cross-compile VIctory on Linux for x86_64-pc-windows-msvc using cargo-xwin, and optionally
# copy victory.exe (+ .pdb) into the folder run.sh starts it from.
#
# Usage:
#   scripts/linux/build-xwin.sh                       # release build
#   scripts/linux/build-xwin.sh --diag                # diag profile + `diagnostics` feature (logs, panel, replay)
#   scripts/linux/build-xwin.sh --deploy-dir <dir>    # also copy the exe there (the old one kept as victory.exe.prev)
#   scripts/linux/build-xwin.sh [flags] -- <extra cargo args>
#
# Environment overrides:
#   REPO_DIR  VIctory git checkout (default: the checkout this script is in)
#   TARGET    Rust target triple   (default: x86_64-pc-windows-msvc)
#
# The checkout's rust-toolchain.toml pins the Rust version and the msvc target;
# rustup installs them on first use. Nothing is committed, pushed or fetched.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="${REPO_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
TARGET="${TARGET:-x86_64-pc-windows-msvc}"

# Ensure rustup's ~/.cargo/bin takes precedence over system /usr/bin/cargo
if [ -d "${HOME}/.cargo/bin" ]; then
    export PATH="${HOME}/.cargo/bin:${PATH}"
fi

DEPLOY=0
DST=""
PROFILE=release
CARGO_ARGS=()

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        --deploy-dir) shift; DST="${1:?--deploy-dir needs a folder}"; DEPLOY=1 ;;
        --diag) PROFILE=diag ;;
        --release) PROFILE=release ;;
        --) shift; CARGO_ARGS+=("$@"); break ;;
        *) echo "ERROR: unknown argument '$1' (use -- before extra cargo args)" >&2; exit 2 ;;
    esac
    shift
done

for tool in cargo cargo-xwin lld-link llvm-lib clang-cl; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        echo "ERROR: Required tool '${tool}' not found in PATH." >&2
        echo "Install prerequisites:" >&2
        echo "  your distribution's lld, llvm, clang and rustup packages" >&2
        echo "  rustup-init -y && source ~/.cargo/env" >&2
        echo "  cargo install cargo-xwin" >&2
        echo "(the target ${TARGET} comes from the checkout's rust-toolchain.toml)" >&2
        exit 1
    fi
done

if [ ! -f "${REPO_DIR}/Cargo.toml" ] || [ ! -f "${REPO_DIR}/.victory-root" ]; then
    echo "ERROR: '${REPO_DIR}' is not a VIctory checkout (Cargo.toml / .victory-root missing)." >&2
    exit 1
fi

if [ "${PROFILE}" = "diag" ]; then
    PROFILE_ARGS=(--profile diag --features diagnostics)
else
    PROFILE_ARGS=(--release)
fi
OUT_DIR="${REPO_DIR}/target/${TARGET}/${PROFILE}"

DIRTY=""
if [ -n "$(git -C "${REPO_DIR}" status --porcelain --untracked-files=no 2>/dev/null)" ]; then
    DIRTY=" (+ uncommitted changes)"
fi

echo "=================================================="
echo "Building VIctory on Linux via cargo-xwin"
echo "Repo:     ${REPO_DIR}"
echo "Commit:   $(git -C "${REPO_DIR}" log -1 --oneline)${DIRTY}"
echo "Profile:  ${PROFILE}"
echo "Target:   ${TARGET}"
echo "Cargo:    $(command -v cargo)"
echo "=================================================="

cd "${REPO_DIR}"
cargo xwin build "${PROFILE_ARGS[@]}" --target "${TARGET}" -p victory "${CARGO_ARGS[@]}"

if [ ! -f "${OUT_DIR}/victory.exe" ]; then
    echo "ERROR: '${OUT_DIR}/victory.exe' not found after build." >&2
    exit 1
fi

if [ "${DEPLOY}" -eq 1 ]; then
    mkdir -p "${DST}"
    echo "Deploying ${PROFILE} build to ${DST}"
    # Keep the previous exe so a bad build can be rolled back.
    if [ -f "${DST}/victory.exe" ] && ! cmp -s "${OUT_DIR}/victory.exe" "${DST}/victory.exe"; then
        cp -f "${DST}/victory.exe" "${DST}/victory.exe.prev"
        echo "  previous victory.exe kept as victory.exe.prev"
    fi
    for f in victory.exe victory.pdb; do
        if [ -f "${OUT_DIR}/${f}" ]; then
            cp -f "${OUT_DIR}/${f}" "${DST}/${f}"
            echo "  copied ${f} ($(stat -c %s "${DST}/${f}") bytes)"
        fi
    done
    chmod +x "${DST}/victory.exe"
    echo "${PROFILE} $(git -C "${REPO_DIR}" log -1 --format=%h)${DIRTY} $(date -Is)" > "${DST}/victory.build"
    echo "  build note written to victory.build"
fi
