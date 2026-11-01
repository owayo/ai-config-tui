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

# インストールして使う場合
cargo install --path . --locked
ai-config-tui
```

## 操作

左にプロファイル、中央に設定、右に説明と値を表示します。幅の狭い端末では説明を下に配置します。最小サイズは60列×16行です。

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

編集画面では`Ctrl+S`で変更を反映し、`Esc`でキャンセルします。文字列は引用符なし、配列などはJSON・TOMLで入力します。ファイルへの保存は、編集画面を閉じた後に`Ctrl+S`を押して確定します。

## 設定先の追加

TUIの`a`で種類・名前・ディレクトリを入力し、`Ctrl+S`で登録します。CLIからも操作できます。

```sh
ai-config-tui profile add --name Work --provider claude --directory ~/.claude-work
ai-config-tui profile add --name Home --provider codex --directory ~/.codex-home
ai-config-tui profile list
ai-config-tui --profile Work
ai-config-tui profile remove Work
```

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
cargo build --release --locked
```
