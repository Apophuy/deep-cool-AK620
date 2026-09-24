#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd -- "$repository_root"
project_version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' Cargo.toml)
if [[ -z "$project_version" ]]; then
    echo "could not read workspace version" >&2
    exit 1
fi
if ! command -v rpmbuild >/dev/null 2>&1; then
    echo "rpmbuild is required; install the rpm build tools" >&2
    exit 1
fi
temporary_root=$(mktemp -d -t ak620-linux-rpm.XXXXXXXX)
trap 'rm -rf -- "$temporary_root"' EXIT
mkdir -p "$temporary_root"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
repository_dir_name=$(basename "$repository_root")
tar \
    --exclude="$repository_dir_name/.git" \
    --exclude="$repository_dir_name/target" \
    --exclude="$repository_dir_name/dist" \
    --transform="s,^$repository_dir_name,ak620-linux-$project_version," \
    -C "$(dirname "$repository_root")" \
    -czf "$temporary_root/SOURCES/ak620-linux-$project_version.tar.gz" \
    "$repository_dir_name"
sed "s/@VERSION@/$project_version/g" packaging/rpm/ak620-linux.spec > "$temporary_root/SPECS/ak620-linux.spec"
rpmbuild \
    --define "_topdir $temporary_root" \
    --define "_dbpath $temporary_root/rpmdb" \
    -bb "$temporary_root/SPECS/ak620-linux.spec"
mkdir -p dist
find "$temporary_root/RPMS" -name '*.rpm' -exec install -Dm644 {} dist/ \;
echo "created RPM package(s) in dist/"
