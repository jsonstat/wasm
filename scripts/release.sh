#!/usr/bin/env bash
# Cut a release for jsonstat-wasm.
#
# This is the local half of the release flow. It does NOT publish to npm
# itself; instead it creates and pushes the `v<version>` git tag, which
# triggers .github/workflows/release.yml to build, verify, publish to npm and
# create the GitHub Release. Keeping the publish step in CI means no npm
# credentials ever live on a developer machine: the workflow authenticates to
# npm via Trusted Publishing (OIDC), which mints a short-lived token per run.
#
# What it does:
#   1. Reads the version from Cargo.toml (single source of truth).
#   2. Sanity-checks the working tree (clean, on main, tag not already used).
#   3. Builds locally (scripts/build.sh) so an obviously broken release is
#      caught before tagging.
#   4. Creates an annotated tag `v<version>` and pushes it to `origin`.
#
# Usage:
#   ./scripts/release.sh             # tag + push v<Cargo.toml version>
#   ./scripts/release.sh --dry-run   # print what would happen, change nothing
#   ./scripts/release.sh --no-build  # skip the local build sanity check
#
# Prerequisites: a clean git tree, push access to `origin`, and the publish
# workflow configured with npm Trusted Publishing (OIDC) (see docs/INSTALL.md).
set -euo pipefail

# ── Config ───────────────────────────────────────────────────────────────
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

REMOTE="${REMOTE:-origin}"
RELEASE_BRANCH="${RELEASE_BRANCH:-main}"

# ── Parse flags ──────────────────────────────────────────────────────────
DRY_RUN=0
RUN_BUILD=1
for arg in "$@"; do
    case "${arg}" in
        --dry-run) DRY_RUN=1 ;;
        --no-build) RUN_BUILD=0 ;;
        -h|--help)
            sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "✗ Unknown argument: ${arg}" >&2
            exit 2
            ;;
    esac
done

run() {
    if [[ "${DRY_RUN}" == "1" ]]; then
        echo "   [dry-run] $*"
    else
        eval "$@"
    fi
}

# ── 1. Read version from Cargo.toml ──────────────────────────────────────
# Match only the package version (line begins with `version = `), not the
# inline `version = "..."` inside dependency tables.
VERSION="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/^version = "(.*)"/\1/')"
if [[ -z "${VERSION}" ]]; then
    echo "✗ Could not read package version from Cargo.toml" >&2
    exit 1
fi
TAG="v${VERSION}"
echo "▶ Releasing ${TAG} (from Cargo.toml)"

# ── 2. Sanity checks ─────────────────────────────────────────────────────
# On the expected release branch?
CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "${CURRENT_BRANCH}" != "${RELEASE_BRANCH}" ]]; then
    echo "✗ On branch '${CURRENT_BRANCH}', expected '${RELEASE_BRANCH}'." >&2
    echo "  Override with RELEASE_BRANCH=... if intentional." >&2
    exit 1
fi

# Clean working tree?
if [[ -n "$(git status --porcelain)" ]]; then
    echo "✗ Working tree is dirty. Commit or stash changes before releasing." >&2
    git status --short >&2
    exit 1
fi

# Tag not already present locally?
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
    echo "✗ Tag ${TAG} already exists locally. Bump the version in Cargo.toml first." >&2
    exit 1
fi

# Tag not already present on the remote?
if git ls-remote --exit-code --tags "${REMOTE}" "refs/tags/${TAG}" >/dev/null 2>&1; then
    echo "✗ Tag ${TAG} already exists on ${REMOTE}. Bump the version in Cargo.toml first." >&2
    exit 1
fi

# ── 3. Local build sanity check ──────────────────────────────────────────
if [[ "${RUN_BUILD}" == "1" ]]; then
    echo "▶ Building locally to validate the release (scripts/build.sh)"
    run "bash scripts/build.sh"
else
    echo "▶ Skipping local build (--no-build)"
fi

# ── 4. Tag + push ────────────────────────────────────────────────────────
echo "▶ Creating annotated tag ${TAG}"
run "git tag -a '${TAG}' -m 'Release ${TAG}'"

echo "▶ Pushing ${TAG} to ${REMOTE}"
run "git push '${REMOTE}' 'refs/tags/${TAG}'"

echo ""
if [[ "${DRY_RUN}" == "1" ]]; then
    echo "✓ Dry run complete — no tag created or pushed."
else
    echo "✓ Pushed ${TAG}."
    echo "  GitHub Actions (.github/workflows/release.yml) will now build,"
    echo "  publish to npm, and create the GitHub Release."
fi
