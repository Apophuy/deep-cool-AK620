.PHONY: check fmt fmt-check lint test doc package-deb package-rpm test-packaging

check: fmt-check lint test doc test-packaging

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	cargo test --workspace --all-features

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

package-deb:
	./packaging/debian/build-deb.sh

package-rpm:
	./packaging/rpm/build-rpm.sh

test-packaging:
	python3 packaging/debian/tests/test_maintainer_scripts.py
