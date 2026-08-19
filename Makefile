PORT ?= 8585

.PHONY: build serve clean

build:
	cd stepper-core && wasm-pack build --target web --out-dir ../www/pkg

serve: build
	cd www && python3 -m http.server $(PORT)

test:
	cd stepper-core && cargo test

clean:
	rm -rf stepper-core/target www/pkg
