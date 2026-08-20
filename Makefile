PORT ?= 8585

.PHONY: build serve test deploy clean

build:
	cd stepper-core && wasm-pack build --target web --out-dir ../www/pkg

serve: build
	cd www && python3 -m http.server $(PORT)

test:
	cd stepper-core && cargo test

# Publish www/ (including the gitignored, generated www/pkg) to the gh-pages
# branch, whose *root* is what GitHub Pages serves. Each deploy is a single
# fresh orphan commit, force-pushed, so the branch never accumulates old .wasm
# blobs in the repo's history.
deploy: build
	git worktree remove --force .deploy 2>/dev/null || true
	rm -rf .deploy
	git branch -D gh-pages 2>/dev/null || true  # last deploy's local branch; remote is the source of truth
	git worktree add --detach .deploy HEAD
	git -C .deploy checkout --orphan gh-pages
	git -C .deploy rm -rf --quiet .
	cp -r www/. .deploy/
	rm -f .deploy/pkg/.gitignore  # wasm-pack writes a '*' ignore that would hide pkg/
	touch .deploy/.nojekyll
	git -C .deploy add -A
	git -C .deploy commit -q -m "Deploy $$(git rev-parse --short HEAD)"
	git -C .deploy push --force origin gh-pages
	git worktree remove --force .deploy

clean:
	rm -rf stepper-core/target www/pkg
