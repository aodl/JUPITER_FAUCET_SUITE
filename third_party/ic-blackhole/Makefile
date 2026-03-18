CANISTERS=blackhole
OBJ=$(CANISTERS:%=dist/%.wasm)
OBJ_OPT=$(CANISTERS:%=dist/%-opt.wasm)
IDL=$(CANISTERS:%=dist/%.did)
NIXPKGS_REV=022caabb5f2265ad4006c1fa5b1ebe69fb0c3faf
NIXPKGS_SHA256=12q00nbd7fb812zchbcnmdg3pw45qhxm74hgpjmshc2dfmgkjh4n

build: $(OBJ) $(IDL) $(OBJ_OPT)

clean:
	rm -rf dist

dist:
	@mkdir -p $@

dist/%.wasm: src/%.mo | dist
	moc -o $@ $<

dist/%-opt.wasm: dist/%.wasm
	wasm-opt -O2 -o $@ $<

dist/%.did: src/%.mo | dist
	moc --idl -o $@ $<

dfx.json:
	@echo '{"canisters":{"blackhole":{"type":"custom","candid":"dist/blackhole.did","wasm":"dist/blackhole-opt.wasm","build":""}}}' > $@

repro-build:
	@nix-build --no-out-link --arg pkgs 'import (builtins.fetchTarball { url = "https://github.com/NixOS/nixpkgs/archive/$(NIXPKGS_REV).tar.gz"; sha256 = "$(NIXPKGS_SHA256)"; }) {}'

.PHONY: build clean really-clean repro-build
