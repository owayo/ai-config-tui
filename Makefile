# ai-config-tui の開発用タスク。引数なしの `make` でターゲット一覧を表示する。
#
# ツールの版は mise.toml が正。mise があればコマンドを `mise exec --` 経由で呼ぶので、
# シェルで mise を activate していなくても (IDE や GUI から make を呼んでも)
# mise.toml の版で動く。mise を使わず PATH 上のツールで動かすなら SYSTEM_TOOLS=1 を
# 付ける (その場合、版の再現性は保証しない)。
#
# macOS 標準の GNU Make 3.81 で動く書き方に限っている
# (.ONESHELL / .SHELLFLAGS / $(file ...) / != は使わない)。

.DEFAULT_GOAL := help

BINARY_NAME := ai-config-tui
INSTALL_PATH ?= /usr/local/bin
# Cargo.lock をコミットしているので、CI と同じく lockfile どおりに依存を解決する
CARGO_FLAGS ?= --locked
BIN_SRC := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target)/release/$(BINARY_NAME)
# root (sudo make install) ではビルドせず、先に一般ユーザーで make release したバイナリを入れる
# (root で cargo を動かすと target/ に root の持ち物が残る)
INSTALL_DEPS := $(if $(filter 0,$(shell id -u)),,release)
# install / uninstall はパスを環境変数で受け取り、シェルの側で展開する (引用符を含むパスでも
# レシピの構文が壊れない)
export INSTALL_PATH BIN_SRC

require_abs = case "$$INSTALL_PATH" in /*) ;; *) printf '%s\n' 'INSTALL_PATH は絶対パスで指定してください' >&2; exit 2;; esac

# ---- ツールチェーン -----------------------------------------------------------
# mise は PATH、よくある導入先の順に探す。GUI から起動した make はシェルの PATH を
# 引き継がないことがあるため。make MISE=/path/to/mise で明示もできる。
# mise が無い環境の振る舞いを試すときは MISE_CANDIDATES= で探す先を空にする。
MISE_CANDIDATES ?= $(HOME)/.local/bin/mise /opt/homebrew/bin/mise /usr/local/bin/mise
# mise.toml のツールを呼ばないターゲット。mise が無くても動かす。install はビルドしない root のときだけ
# (sudo の環境には一般ユーザーの mise が見えないことがある)。
# この行に並べる (下の判定は make がファイルを読む時点で動くので、後ろで足しても効かない)
NO_MISE_TARGETS := help uninstall $(if $(INSTALL_DEPS),,install)
ifeq ($(SYSTEM_TOOLS),1)
RUN :=
else
ifndef MISE
MISE := $(firstword $(shell command -v mise 2>/dev/null) $(wildcard $(MISE_CANDIDATES)))
endif
ifeq ($(MISE),)
ifneq ($(filter-out $(NO_MISE_TARGETS),$(or $(MAKECMDGOALS),help)),)
$(error mise が見つかりません。https://mise.jdx.dev で導入するか、PATH 上のツールで実行するなら SYSTEM_TOOLS=1 を付けてください)
endif
endif
RUN := $(if $(MISE),$(MISE) exec --,)
endif

.PHONY: help setup build release run test catalog-test lint fmt fmt-check check ci install uninstall clean catalog

## セットアップ

setup: ## ツールチェーン (mise) と依存を取得する
	@if [ -n "$(MISE)" ]; then "$(MISE)" install; fi
	$(RUN) cargo fetch $(CARGO_FLAGS)

## ビルド

build: ## デバッグ版をビルドする
	$(RUN) cargo build $(CARGO_FLAGS)

release: ## リリース版をビルドする
	$(RUN) cargo build --release $(CARGO_FLAGS)

run: ## デバッグ版を実行する (引数は ARGS="...")
	$(RUN) cargo run $(CARGO_FLAGS) -- $(ARGS)

## 検査

# CI が cargo test / clippy に追加のフラグを渡すなら、ここにも同じものを付ける
test: catalog-test ## テストを実行する (カタログ生成器のテストも含む)
	$(RUN) cargo test $(CARGO_FLAGS)

catalog-test: ## カタログ生成器のテストを実行する (通信は要らない)
	$(RUN) python3 -m unittest discover -s scripts -p 'test_*.py'

lint: ## clippy を警告ゼロで通す
	$(RUN) cargo clippy $(CARGO_FLAGS) --all-targets -- -D warnings

fmt: ## コードを整形する (書き換える)
	$(RUN) cargo fmt --all

fmt-check: ## 整形済みかを確かめる (書き換えない)
	$(RUN) cargo fmt --all -- --check

check: fmt-check lint ## 整形と静的検査 (書き換えない)

ci: check test ## CI と同じ検査 (書き換えない)

## インストール

# 動いているバイナリ (や直前まで動いていたバイナリ) を cp で上書きせず、同じディレクトリの一時ファイルから
# rename で置き換える。macOS はコード署名の検証結果を inode ごとに覚えているので、上書きしたバイナリは
# 起動の直後に SIGKILL (exit 137) される。署名は付け直さない (リンカが ad-hoc で署名済み)
install: $(INSTALL_DEPS) ## リリース版を INSTALL_PATH (既定 /usr/local/bin) に入れる
	@$(require_abs)
	@test -f "$$BIN_SRC" || { printf '%s\n' 'リリース版のバイナリがありません。先に make release を実行してください' >&2; exit 1; }
	@if [ -d "$$INSTALL_PATH/$(BINARY_NAME)" ]; then printf 'インストール先が既にディレクトリです: %s\n' "$$INSTALL_PATH/$(BINARY_NAME)" >&2; exit 1; fi
	@mkdir -p "$(INSTALL_PATH)"
	@set -eu; \
	install_tmp=$$(mktemp "$$INSTALL_PATH/.$(BINARY_NAME).XXXXXX"); \
	trap 'rm -f "$$install_tmp"' 0; \
	trap 'exit 1' HUP INT TERM; \
	cp "$$BIN_SRC" "$$install_tmp"; \
	chmod 755 "$$install_tmp"; \
	mv -f "$$install_tmp" "$$INSTALL_PATH/$(BINARY_NAME)"; \
	printf 'インストールしました: %s\n' "$$INSTALL_PATH/$(BINARY_NAME)"

# 設定ファイル・プロファイルの登録・バックアップは消さない
uninstall: ## INSTALL_PATH から取り除く
	@$(require_abs)
	rm -f "$(INSTALL_PATH)/$(BINARY_NAME)"

clean: ## ビルド成果物を消す
	$(RUN) cargo clean

## カタログ

catalog: ## 公式資料から設定の一覧を作り直す (通信が要る)
	$(RUN) python3 scripts/update_catalog.py

## ヘルプ

help: ## このヘルプを表示する
	@echo "$(BINARY_NAME) の開発用タスク"
	@echo ""
	@echo "使い方: make <target>"
	@echo ""
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "ツールの版は mise.toml を参照。初回は make setup"
	@echo "インストール先を変える: make install INSTALL_PATH=\"\$$HOME/.local/bin\""
	@echo "リリース: GitHub Actions > Release > Run workflow"
