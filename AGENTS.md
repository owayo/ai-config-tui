# ai-config-tui — エージェント向けガイド

このリポジトリで作業する AI エージェント (と人間の開発者) 向けの案内。利用者向けの説明は [README.md](README.md) にある (日本語だけで書く。英語版は置かない)。

## プロジェクト概要

ai-config-tui は、Claude Code (`settings.json`) と Codex (`config.toml`) の設定を編集する Rust 製の TUI。設定ディレクトリを「プロファイル」として登録し、切り替えて編集する。画面の文言・CLI のヘルプ・設定の説明はすべて日本語で書く。

通信はしない。公式の設定の一覧と日本語の説明は `data/` の JSON に置き、`include_str!` でバイナリに埋め込む (`src/catalog.rs`)。

設定ファイルの値は自動で書き換えない。未知のキーと TOML のコメントを残し、保存の前に構文と外部からの変更を確かめる。

## 技術スタック

- Rust (edition 2024)。ツールチェーンの版は `mise.toml` が正 (`Cargo.toml` の `rust-version` は mise の rust の `major.minor` にそろえる)
- TUI: `ratatui` + `crossterm`、CLI: `clap`
- 設定ファイル: `serde_json` (`preserve_order` でキーの順を保つ)、`toml_edit` (コメントと書式を保つ)
- カタログの生成器: Python (標準ライブラリだけ) と curl。Python の版も `mise.toml` で固定している

## 構成

| パス | 役割 |
|---|---|
| `src/main.rs` | CLI の定義 (`profile` サブコマンド) と TUI の起動 |
| `src/app.rs` | TUI の状態とキー操作 (フォーカス、モーダル、候補の選択) |
| `src/ui.rs` | 描画 |
| `src/profile.rs` | プロファイルの登録 (`profiles.toml`)、種類の判定、環境変数からの追加 |
| `src/document.rs` | JSON / TOML の読み込み・編集・取り消し・書き出し |
| `src/fields.rs` | 設定の項目の一覧 (カタログと実際のファイルのキーを合わせる)、表示用の値、検索 |
| `src/catalog.rs` | `data/` の 3 つの JSON の読み込み |
| `src/storage.rs` | 保存 (外部変更の検査、一時ファイルからの置き換え、バックアップの世代管理、Unix の権限) |
| `src/editor.rs` | テキストの入力欄 |
| `src/tests.rs` | 単体テスト (TUI の描画と操作を含む) |
| `tests/cli.rs` | バイナリを起動する結合テスト |
| `tests/install.rs` | Unix 向けのインストール・削除テスト。保存先名をシェルに解釈させないことも確認する |
| `data/settings-catalog.json` | 公式資料から `make catalog` が作る設定の一覧。手で書き換えない |
| `data/settings-help.ja.json` | 設定の日本語名と説明。手で管理する |
| `data/choice-help.ja.json` | 候補ごとの説明。グループごとに出典の URL を持ち、全体の確認日を `verified_at` に書く |
| `scripts/update_catalog.py` | カタログの生成器 (`make catalog` が呼ぶ)。テストは `scripts/test_update_catalog.py` |

## テストとチェック

```bash
make setup         # 初回: mise.toml のツールと依存を入れる
make ci            # CI の quality ジョブと同じ検査 (fmt-check、clippy、カタログ生成器のテスト、cargo test)
make test          # テストだけ (カタログ生成器のテストも含む)
make catalog-test  # カタログ生成器のテストだけ (通信は要らない)
make fmt           # 整形 (ファイルを書き換える)
```

- 変更したら `make ci` を通してから終える
- CI は Linux と macOS で `make ci` を回し、Windows では build のジョブで `cargo test` と `cargo clippy` を直接呼ぶ。`#[cfg(unix)]` で分けたコード (`src/storage.rs` の権限など) を変えたら、Windows 向けの clippy も手元で確かめる

  ```bash
  mise exec -- rustup target add x86_64-pc-windows-msvc
  mise exec -- cargo clippy --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings
  ```

- テストは利用者の設定に触れない。一時ディレクトリを `HOME`・`XDG_CONFIG_HOME`・`XDG_STATE_HOME` に渡し、`CLAUDE_CONFIG_DIR` と `CODEX_HOME` を外す。プロファイルの登録先は `--registry` で一時ディレクトリを指す (`tests/cli.rs`)。Windows の `dirs::home_dir()` は `HOME` を見ないので、パスは引数で渡す作りにする
- 環境変数を使うテストは `Command::env` で子プロセスだけに渡し、テストプロセスの環境を変更しない。環境変数からの追加でも、通常の登録と同じディレクトリ照合を使い、プロファイル名の一意性を保つ
- プロファイルの由来は `ProfileOrigin` で区別し、登録由来だけを保存する。起動時には登録情報の名前を検証し、ディレクトリの不整合は項目を開くときや一覧表示時に検査する。壊れた登録があっても解除できることを保つ
- マスク対象は検索前の設定一覧で判定し、秘密情報を含む親項目にも引き継ぐ。JSON の無変更の項目編集では元の書式を保ち、キー削除には順序を保つ API を使う。TOML のキー削除で移し先のないコメントは文書末尾に残す
- `tests/install.rs` は一時ディレクトリで `make` を動かす。ビルドとツールチェーンの呼び出しを省き、テスト用ファイルのコピー・削除だけを検査する。`INSTALL_PATH` は環境変数でレシピに渡し、シェル内では引用した `$$INSTALL_PATH` を使う
- 設定一覧の性能計測は `mise exec -- cargo test --release --locked benchmark_settings_rows -- --ignored --nocapture` で実行する。各プロバイダーで追加 0 / 1,000 項目を使い、5 回の準備実行後に 25 回計測する。時間の上限は通常のテストに設けない

## 設定の説明を足すとき

- 設定の日本語名と説明は `data/settings-help.ja.json`、候補の説明は `data/choice-help.ja.json` に足す。公式資料で確かめた内容だけを書き、推測で補わない。確かめられない候補は説明を登録しない (画面には「説明未登録」と出る)
- 候補の説明は、プロバイダー・設定のパス・JSON の値と型で照合する。同じ `auto` や `none` でも、設定ごとに意味を書く
- Codex のスキーマでは参照先の型と、`$ref`・`allOf` の隣のプロパティ・既定値も読む。任意名・配列要素・深い子項目をたどり、循環は参照経路で止める。`<index>` を名前付きテーブルと照合しない
- 公式表の代入例 (`= []` など) をキーに含めない。テーブルで指定する方式名を文字列候補にしない。旧 `profiles` 表の子は展開せず、現行の名前付きレイヤーが別ファイルであることを記録する
- JSON の未知の数値の精度と、TOML の行末コメントを保つ。表示用の値では TOML の外側の装飾を除く。ヘッダーの値は名前にかかわらずマスクし、公式定義が数値・真偽値の項目だけを実際の型との一致で除外する
- `data/settings-catalog.json` は `make catalog` で作り直す (通信が要る)。生成器を変えたら `make catalog-test` を通す

## リリース

リリースは GitHub Actions の **Release** を **Run workflow** で動かす。まず `dry_run` で次の版を確かめる。本番は、そのコミットの `ci.yml` (push の実行) が成功していないと最初のジョブで止まる。版は JST の `yy.m.counter` (例 `26.11.100`) で、ワークフローが `Cargo.toml` と `Cargo.lock` を書き換えてタグを付けるので、版を手で変えない。

build 以降が落ちたら、同じ実行の **Re-run failed jobs** でやり直す。**Re-run all jobs** は同じ番号を採り直し、タグが既にあるので最初のジョブで止まる。

配布物は macOS (Apple Silicon) の `.tar.gz` と Windows (x64) の `.zip` で、Homebrew の tap (`owayo/homebrew-ai-config-tui`) の formula もワークフローが書き直す。dry_run では build が動かないので、`ci.yml` の build と `release.yml` の build を同じ組 (os と target) にしておき、配布物を作れることを CI の build で確かめる。

## 公開リポジトリとしての注意

public のリポジトリなので、社内の情報、個人のパス、実在のメールアドレスを書かない。README・テスト・例のプロファイル名は `Work` / `Home` のような一般の名前にする。
