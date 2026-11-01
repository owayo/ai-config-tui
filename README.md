# ai-config-tui

Claude CodeとCodexの設定を編集する、Rust製のTUIです。設定先を追加登録し、プロファイルを切り替えて編集できます。ネットワーク接続やAPIキーは不要です。

| 種類 | 既定の編集先 |
| --- | --- |
| Claude Code | `~/.claude/settings.json` |
| Codex | `~/.codex/config.toml` |

## 起動

Rust 1.89以降と、日本語・UTF-8を表示できる端末が必要です。

```sh
cargo run --locked

# /usr/local/binへインストール
make install
ai-config-tui

# インストール先を変更する場合
make install INSTALL_PATH="$HOME/.local/bin"

# 指定先のバイナリを削除（設定ファイルは保持）
make uninstall INSTALL_PATH="$HOME/.local/bin"
```

`make install`はリリースビルド後にバイナリを配置します。上書き時は同じディレクトリの一時ファイルから置き換えます。`make help`でターゲット一覧を確認できます。Cargoで管理する場合は`cargo install --path . --locked`も使えます。

## 操作

左にプロファイル、中央に設定、右に説明と値を表示します。プロファイルはClaude Code、Codexの順にまとめ、各種類の中では既定・登録順に並べます。上下キーと`[`・`]`もこの表示順で切り替わります。幅の狭い端末では説明を下に配置します。最小サイズは60列×16行です。

| キー | 操作 |
| --- | --- |
| `Tab` / `←` / `→` | ペイン切替 |
| `↑` / `↓`、`j` / `k` | 移動 |
| `[` / `]` | プロファイル切替 |
| `Enter` / `e` | 設定を編集 |
| `Space` | 真偽値を切替 |
| `E` / `F4` | JSON・TOML全体を編集 |
| `d` / `u` | 未設定に戻す / 直前の変更を取り消す |
| `Ctrl+S` / `s` | 差分を確認して保存 |
| `/` / `Esc` | 検索 / 検索解除 |
| `a` | プロファイルを追加 |
| `x` | 左ペインで登録を解除 |
| `r` / `h` | 再読み込み / 値のマスク切替 |
| `?` / `q` | ヘルプ / 終了 |

モデル・権限モード・テーマ・真偽値などは、`Enter`で候補を開き、`↑↓`と`Enter`で選びます。Claudeのモデルは`opus`・`sonnet`・`haiku`などから選択できます。モデルIDや任意の文字列には自由入力も使えます。利用可能なモデルや推論レベルは、契約・CLI・プロバイダーによって異なります。

選択中の候補には、確認できた範囲で公式資料を基にした説明・参照URL・確認日を表示します。たとえばCodexの`web_search`では、`cached`は索引の保存済み結果を利用し、`live`はWebから最新情報を取得することを説明します。長い説明は`Ctrl+↑↓`でスクロールできます。候補の移動だけでは設定を変更しません。

選択画面の「未設定」はキーの削除、`Esc`は変更なしです。既定値の表示は公式資料の値で、他の設定ファイルを合成した実効値ではありません。

テキストの編集画面では`Ctrl+S`で変更を反映し、`Esc`でキャンセルします。文字列は引用符なし、配列などはJSON・TOMLで入力します。ファイルへの保存は、編集画面を閉じた後に`Ctrl+S`を押して確定します。

## 公式設定の一覧

2026年11月1日に確認した公式資料を基に、Claudeの247項目、Codexの953項目の定義を同梱しています。CodexはリファレンスとJSONスキーマを合わせた一覧です。未設定の項目も表示し、詳細ペインに適用範囲・出典・参照日を示します。設定先を開くだけでは、未設定のキーをファイルに書き込みません。

設定キーと日本語の設定名を併記し、確認できた項目には用途の説明を表示します。キー・設定名・説明のどれでも検索できます。日本語の名前と説明は本ツールで付けたもので、[data/settings-help.ja.json](data/settings-help.ja.json)で管理します。専用の名前がない項目は階層名と読みやすく区切ったキーを使い、説明を推測で補いません。

候補ごとの説明の管理先は[data/choice-help.ja.json](data/choice-help.ja.json)です。プロバイダー・設定のパス・JSONの値と型で照合し、同じ`auto`や`none`でも設定ごとの意味を示します。未確認の候補は説明未登録と表示し、未設定・現在値の保持・自由入力には本ツールでの操作内容を表示します。

Claude Code・Codexとも、親の下に子をまとめた階層表示です。たとえば次のように表示し、詳細欄には完全なキーを示します。検索時も一致する子の親を表示します。表示用の中間グループは編集できず、値を持つ設定はこれまでどおり編集できます。

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

廃止・無効な設定と非推奨の設定は、未設定なら検索結果も含めて非表示です。設定済みなら完全なキーと状態を示し、廃止は赤色、非推奨はオレンジ色で表示します。並びは「廃止→非推奨→通常の設定」です。`false`・空文字・空配列・`null`も設定済みとして扱います。

値は自動で変更せず、非推奨の設定は編集もできます。`d`で未設定にすると一覧から消え、`u`で取り消すと再表示されます。

管理者専用、別ファイル向け、廃止済みの設定は参照表示です。名前が可変の項目は`<name>`などのテンプレートで表示し、設定済みの項目は具体名でも表示します。新しい名前の定義には`E`の全体編集を使えます。

一覧の更新は`make catalog`です。Python 3.11以降とcurlで公式資料を取得し、キー・型・選択肢・適用範囲と取得元のSHA-256を[data/settings-catalog.json](data/settings-catalog.json)へ保存します。通常の起動・ビルドでは同梱データを使います。

## 設定先の追加

TUIの`a`で名前とディレクトリを入力し、`Ctrl+S`で登録します。種類の初期値は自動判定です。`config.toml`があればCodex、`settings.json`があればClaude Codeとして判定します。両方ある場合は手動で種類を選択してください。ファイルがまだなければ`.codex-work`・`.claude-work`などのディレクトリ名で判定し、それでも決まらなければ種類の選択を求めます。追加画面には登録先の設定ファイルも表示します。

CLIでも`--provider`を省略すると自動判定します。手動で指定した種類は自動判定より優先します。

```sh
ai-config-tui profile add --name Work --provider claude --directory ~/.claude-work
ai-config-tui profile add --name Home --provider codex --directory ~/.codex-home
ai-config-tui profile list
ai-config-tui profile set-provider Work --provider codex
ai-config-tui --profile Work
ai-config-tui profile remove Work
```

登録済みの種類を修正する場合は`profile set-provider`を使います。変更するのは登録情報で、Claude Code・Codexの設定ファイルはそのまま保持します。

登録先は`~/.config/ai-config-tui/profiles.toml`です。`XDG_CONFIG_HOME`と`--registry PATH`でも変更できます。`CLAUDE_CONFIG_DIR`・`CODEX_HOME`の指定先も一覧に追加します。環境変数から追加された設定先は、登録を解除しても次回起動時に再表示されます。

このツールのプロファイルは「設定ディレクトリ」の登録で、Codex自身の`--profile`とは別の機能です。登録を解除しても設定ファイルは残ります。

## 保存と復旧

未保存の編集はプロファイルを切り替えても保持し、終了時に確認します。未知のキーを保持し、TOMLの項目編集ではコメントも残します。JSONの項目編集ではインデントを2文字に整形します。

保存前に構文と外部変更を検査します。バックアップは`~/.local/state/ai-config-tui/backups/`にファイルごとに直近10世代を保存し、保存後にパスを表示します。`XDG_STATE_HOME`にも対応します。復旧する場合は、対象のバックアップを元の設定ファイルへコピーしてください。

同じディレクトリ内の一時ファイルから置き換え、シンボリックリンクと既存のファイル権限を保持します。Unixでは新規ファイルとバックアップを`0600`で作成します。他のツールとの同時書き込みを完全に排他するものではありません。

検証はJSON・TOMLの構文が対象です。設定値の対応状況や管理者設定による優先順位は、各CLIのバージョンで異なります。設定ファイルはUTF-8、5 MiB以内に対応します。

`env`や秘密情報を示す名前のキーは通常画面でマスクします。編集画面・全体編集・保存差分には値を表示します。認証ファイルは編集対象に含めません。

設定キーの参照: [Claude Code公式ドキュメント](https://code.claude.com/docs/en/settings-reference)、[Codex公式ドキュメント](https://learn.chatgpt.com/docs/config-file/config-reference)。

## 開発

```sh
make check
make build

# カタログ生成器のテスト（Python 3.11以降、ネットワーク不要）
python3 -m unittest discover -s scripts -p 'test_*.py'
```
