<h1 align="center">ai-config-tui</h1>

<p align="center">
  Claude Code と Codex の設定を、公式の設定一覧と日本語の説明を見ながら、プロファイルごとに編集できる TUI
</p>

<!-- standard:badges:start -->
<h3 align="center">対応プラットフォーム</h3>

<p align="center">
  <img src="https://img.shields.io/badge/macOS-000000?logo=apple&amp;logoColor=white" alt="macOS">
  <img src="https://img.shields.io/badge/Windows-0078D6" alt="Windows">
</p>

<p align="center">
  <a href="https://github.com/owayo/ai-config-tui/actions/workflows/ci.yml"><img src="https://github.com/owayo/ai-config-tui/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="https://github.com/owayo/ai-config-tui/releases/latest"><img src="https://img.shields.io/github/v/release/owayo/ai-config-tui" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/owayo/ai-config-tui" alt="License"></a>
</p>
<!-- standard:badges:end -->

---

Claude Code の `settings.json` と Codex の `config.toml` を、端末の画面で編集する Rust 製の TUI です。公式資料から作った設定の一覧を同梱しているので、ファイルにまだ書いていない項目も日本語の設定名を添えて表示します。

設定先 (設定ディレクトリ) を追加で登録すると、プロファイルを切り替えて編集できます。ネットワーク接続や API キーは不要です。

## 機能

- **公式設定の一覧**: Claude Code と Codex の公式資料を基にした設定の定義を同梱し、未設定の項目も表示します。キー・日本語の設定名・説明のどれでも検索できます。
- **候補の説明**: モデル・権限モード・テーマ・真偽値などは候補から選べます。選択中の候補には、確認できた範囲で公式資料を基にした説明と参照 URL を表示します。
- **廃止・非推奨の表示**: 設定済みの廃止の設定は赤色、非推奨の設定はオレンジ色で示します。値は自動で変更しません。
- **階層表示**: 親の下に子をまとめて表示します。検索の結果でも、一致した子の親を表示します。
- **複数の設定先**: 設定ディレクトリを登録し、プロファイルを切り替えて編集します。`CLAUDE_CONFIG_DIR`・`CODEX_HOME` の指定先も一覧に加えます。
- **保存前の確認とバックアップ**: 差分を確かめてから保存し、構文と外部変更を保存前に検査します。バックアップは設定ファイルごとに直近 10 世代を残します。

## 動作環境

日本語と UTF-8 を表示できる端末が必要です。端末の大きさは最小 60 列×16 行です。

配布物 (Homebrew と GitHub Releases の添付) は、macOS の Apple Silicon 向けと Windows x64 (x86_64) 向けだけです。Intel の Mac と Linux 向けの配布物はありません。

## インストール

<!-- standard:install:start -->
### Homebrew (macOS)

```bash
brew install owayo/ai-config-tui/ai-config-tui
```

### Cargo

Rust 1.99 以上が必要です。

```bash
cargo install --git https://github.com/owayo/ai-config-tui --locked
```

### GitHub Releases から

[Releases](https://github.com/owayo/ai-config-tui/releases/latest) から自分の環境のアーカイブを取得して展開し、`ai-config-tui` を `PATH` の通った場所に置きます。各リリースには、取得したファイルを確かめるための `SHA256SUMS` も添付しています。

| プラットフォーム | ファイル |
|---|---|
| macOS (Apple Silicon) | `ai-config-tui-aarch64-apple-darwin.tar.gz` |
| Windows (x86_64) | `ai-config-tui-x86_64-pc-windows-msvc.zip` |

macOS でブラウザから取得した場合は、実行の前に隔離属性を外します: `xattr -d com.apple.quarantine ai-config-tui`。

### ソースから

[mise](https://mise.jdx.dev/) が必要です (Rust のツールチェーンは `mise.toml` で固定しています)。

```bash
git clone https://github.com/owayo/ai-config-tui.git
cd ai-config-tui
make install
```

`make install` は `/usr/local/bin` に入れます。場所を変えるときは `INSTALL_PATH` を指定します (例: `make install INSTALL_PATH="$HOME/.local/bin"`)。
<!-- standard:install:end -->

### 更新と削除

Homebrew で入れた場合は、`brew upgrade ai-config-tui` で更新し、`brew uninstall ai-config-tui` で削除します。

ソースから入れた場合、`make install` をもう一度実行すると、同じディレクトリの一時ファイルからバイナリを置き換えます。削除は `make uninstall` です。指定先のバイナリだけを消し、設定ファイルは残します。インストール先を変えた場合は、同じ `INSTALL_PATH` を指定してください。

```bash
make uninstall INSTALL_PATH="$HOME/.local/bin"
```

## 使い方

### 起動

```bash
ai-config-tui
```

`--profile <名前>` を付けると、登録済みのプロファイルを選んだ状態で起動します。

既定の編集先は次のとおりです。

| 種類 | 既定の編集先 |
|---|---|
| Claude Code | `~/.claude/settings.json` |
| Codex | `~/.codex/config.toml` |

画面の左にプロファイル、中央に設定、右に説明と値を表示します。幅の狭い端末では説明を下に配置します。プロファイルは Claude Code、Codex の順にまとめ、各種類の中では既定・登録順に並べます。上下キーと `[`・`]` もこの表示順で切り替わります。

### キー操作

| キー | 操作 |
|---|---|
| `Tab` / `←` / `→` | ペイン切替 |
| `↑` / `↓`、`j` / `k` | 移動 |
| `[` / `]` | プロファイル切替 |
| `Enter` / `e` | 設定を編集 |
| `Space` | 真偽値を切替 |
| `E` / `F4` | JSON・TOML 全体を編集 |
| `d` / `u` | 未設定に戻す / 直前の変更を取り消す |
| `Ctrl+S` / `s` | 差分を確認して保存 |
| `/` / `Esc` | 検索 / 検索解除 |
| `a` | プロファイルを追加 |
| `x` | 左ペインで登録を解除 |
| `r` / `h` | 再読み込み / 値のマスク切替 |
| `?` / `q` | ヘルプ / 終了 |

### 値の選択と編集

モデル・権限モード・テーマ・真偽値などは、`Enter` で候補を開き、`↑↓` と `Enter` で選びます。Claude のモデルは `opus`・`sonnet`・`haiku` などから選択できます。モデル ID や任意の文字列には自由入力も使えます。利用可能なモデルや推論レベルは、契約・CLI・プロバイダーによって異なります。

選択中の候補には、確認できた範囲で公式資料を基にした説明・参照 URL・確認日を表示します。たとえば Codex の `web_search` では、`cached` は索引の保存済み結果を利用し、`live` は Web から最新情報を取得することを説明します。長い説明は `Ctrl+↑↓` でスクロールできます。候補を移動しただけでは、設定は変わりません。

選択画面の「未設定」はキーの削除、`Esc` は変更なしです。既定値の表示は公式資料の値で、他の設定ファイルを合成した実効値ではありません。

テキストの編集画面では `Ctrl+S` で変更を反映し、`Esc` でキャンセルします。文字列は引用符なし、配列などは JSON・TOML で入力します。ファイルに書き込むのは、編集画面を閉じてから `Ctrl+S` を押したときです。

### 設定先の追加

TUI では `a` で名前とディレクトリを入力し、`Ctrl+S` で登録します。追加画面には登録先の設定ファイルも表示します。

種類の初期値は自動で判定します。`config.toml` があれば Codex、`settings.json` があれば Claude Code です。両方ある場合は手動で種類を選択してください。ファイルがまだなければ `.codex-work`・`.claude-work` などのディレクトリ名で判定し、それでも決まらなければ種類の選択を求めます。

CLI でも `--provider` を省略すると自動で判定します。手動で指定した種類は自動判定より優先します。

```bash
ai-config-tui profile add --name Work --provider claude --directory "$HOME/.claude-work"
ai-config-tui profile add --name Home --provider codex --directory "$HOME/.codex-home"
ai-config-tui profile list
ai-config-tui profile set-provider Work --provider codex
ai-config-tui --profile Work
ai-config-tui profile remove Work
```

登録済みの種類を直すには `profile set-provider` を使います。変わるのは登録情報だけで、Claude Code・Codex の設定ファイルはそのまま残ります。

## 公式設定の一覧

2026 年 11 月 1 日に確認した公式資料を基に、Claude Code の 247 項目、Codex の 953 項目の定義を同梱しています。Codex の分はリファレンスと JSON スキーマを合わせた一覧です。未設定の項目も表示し、詳細ペインに適用範囲・出典・参照日を示します。設定先を開くだけでは、未設定のキーをファイルに書き込みません。

設定キーと日本語の設定名を併記し、確認できた項目には用途の説明を表示します。キー・設定名・説明のどれでも検索できます。日本語の名前と説明は本ツールで付けたものです。専用の名前がない項目は階層名と読みやすく区切ったキーを使い、説明を推測で補いません。

候補の説明は設定ごとに引き当てるので、同じ `auto` や `none` でも、その設定での意味を示します。未確認の候補は説明未登録と表示し、未設定・現在値の保持・自由入力には本ツールでの操作内容を表示します。

Claude Code・Codex とも、親の下に子をまとめた階層表示です。たとえば次のように表示し、詳細欄には完全なキーを示します。検索時も一致する子の親を表示します。表示用の中間グループは編集できず、値を持つ設定は通常どおり編集できます。

```text
permissions
├─ defaultMode
├─ allow
└─ deny
sandbox
└─ network
   └─ allowedDomains
browser_use
└─ allow_history_access
```

廃止・無効な設定と非推奨の設定は、未設定なら検索結果も含めて非表示です。設定済みなら完全なキーと状態を示し、廃止は赤色、非推奨はオレンジ色で表示します。並びは「廃止→非推奨→通常の設定」です。`false`・空文字・空配列・`null` も設定済みとして扱います。

値は自動で変更せず、非推奨の設定は編集もできます。`d` で未設定にすると一覧から消え、`u` で取り消すと再表示されます。

管理者専用、別ファイル向け、廃止済みの設定は参照表示です。名前が可変の項目は `<name>` などのテンプレートで表示し、設定済みの項目は具体名でも表示します。新しい名前の定義には `E` の全体編集を使えます。

設定キーの参照: [Claude Code 公式ドキュメント](https://code.claude.com/docs/en/settings-reference)、[Codex 公式ドキュメント](https://learn.chatgpt.com/docs/config-file/config-reference)。

## 保存と復旧

未保存の編集はプロファイルを切り替えても消えず、終了時に確認します。未知のキーは保持し、TOML の項目編集ではコメントも残します。JSON の項目編集ではインデントを 2 文字に整形します。

保存前に構文と外部変更を検査します。バックアップはファイルごとに直近 10 世代を `~/.local/state/ai-config-tui/backups/` に残し、保存後にそのパスを表示します (`XDG_STATE_HOME` にも対応)。復旧するときは、対象のバックアップを元の設定ファイルへコピーしてください。

保存では同じディレクトリ内の一時ファイルから置き換え、シンボリックリンクと既存のファイル権限を保持します。Unix では新規ファイルとバックアップを `0600` で作成します。他のツールとの同時書き込みを完全に排他するものではありません。

検証は JSON・TOML の構文が対象です。設定値の対応状況や管理者設定による優先順位は、各 CLI のバージョンで異なります。設定ファイルは UTF-8、5 MiB 以内に対応します。

`env` や秘密情報を示す名前のキーは、通常画面でマスクします。編集画面・全体編集・保存差分には値を表示します。認証ファイルは編集対象に含めません。

## 設定

設定先の登録は `~/.config/ai-config-tui/profiles.toml` に保存します。保存先は `XDG_CONFIG_HOME` と `--registry PATH` でも変えられます。参照するオプションと環境変数は次のとおりです。

| 指定 | 効果 |
|---|---|
| `--registry PATH` | 登録の保存先を `PATH` にする |
| `XDG_CONFIG_HOME` | 登録の保存先を `$XDG_CONFIG_HOME/ai-config-tui/profiles.toml` にする |
| `CLAUDE_CONFIG_DIR` | 指定先を Claude Code の設定先として一覧に加える |
| `CODEX_HOME` | 指定先を Codex の設定先として一覧に加える |
| `XDG_STATE_HOME` | バックアップを `$XDG_STATE_HOME/ai-config-tui/backups/` に置く |

環境変数から追加された設定先は、登録を解除しても次回起動時に再表示されます。

このツールのプロファイルは「設定ディレクトリ」の登録で、Codex 自身の `--profile` とは別の機能です。登録を解除しても設定ファイルは残ります。

## 開発

<!-- standard:dev:start -->
[mise](https://mise.jdx.dev/) が必要です。ツールの版は `mise.toml` で固定しています。

```bash
make setup   # ツールチェーン (mise) と依存を取得する
make ci      # CI と同じ検査 (書き換えない)
```

| コマンド | 説明 |
|---|---|
| `make setup` | ツールチェーン (mise) と依存を取得する |
| `make build` | デバッグ版をビルドする |
| `make release` | リリース版をビルドする |
| `make run` | デバッグ版を実行する (引数は ARGS="...") |
| `make test` | テストを実行する |
| `make lint` | clippy を警告ゼロで通す |
| `make fmt` | コードを整形する (書き換える) |
| `make fmt-check` | 整形済みかを確かめる (書き換えない) |
| `make check` | 整形と静的検査 (書き換えない) |
| `make ci` | CI と同じ検査 (書き換えない) |
| `make install` | リリース版を INSTALL_PATH (既定 /usr/local/bin) に入れる |
| `make uninstall` | INSTALL_PATH から取り除く |
| `make clean` | ビルド成果物を消す |

`make` でターゲットの一覧を表示します。リリースは GitHub Actions で行います (**Actions → Release → Run workflow**)。
<!-- standard:dev:end -->

### 公式設定の一覧の更新

同梱の一覧は `make catalog` で公式資料から作り直します。curl と通信が要ります (Python は `mise.toml` で固定しています)。通常の起動とビルドでは同梱のデータを使います。

```bash
make catalog        # 公式資料から設定の一覧を作り直す (通信が要る)
make catalog-test   # カタログ生成器のテストを実行する (通信は要らない)
```

カタログ生成器のテストは `make test` と `make ci` にも含まれます。

### データファイル

| ファイル | 中身 |
|---|---|
| [data/settings-catalog.json](data/settings-catalog.json) | 公式資料から取得したキー・型・選択肢・適用範囲と、取得元の SHA-256。`make catalog` が書き出す |
| [data/settings-help.ja.json](data/settings-help.ja.json) | 日本語の設定名と説明 |
| [data/choice-help.ja.json](data/choice-help.ja.json) | 候補ごとの説明。プロバイダー・設定のパス・JSON の値と型で照合する |

## ライセンス

<!-- standard:license:start -->
[MIT](LICENSE)
<!-- standard:license:end -->
