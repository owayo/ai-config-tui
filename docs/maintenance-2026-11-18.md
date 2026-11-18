# 2026-11-18 の定期点検

Claude Code 2.1.327 と Codex 0.169.1 を対象に、公式資料と同梱の設定の一覧・日本語の説明・編集画面を照合した。どちらも点検日の最新版で、Claude Code は更新履歴の最新、Codex は公開版 rust-v0.169.1 (2026-11-17 公開) と一致する。利用者の設定と登録済みのプロファイルは使っていない。11 月 13 日の点検で途中まで進んでいた変更 (キーの正規表記) も、この点検で仕上げた。

## 公式資料と範囲

取得元 8 件と SHA-256 は[カタログ](../data/settings-catalog.json)の `sources`・`source_sha256` にある。取得は 2026-11-18 で、転送 (リダイレクト) はなかった。資料を固定した生成と、取得し直した生成の結果は一致した。

- Claude Code: [設定](https://code.claude.com/docs/en/settings-reference)、[優先順位](https://code.claude.com/docs/en/settings)、[モデル](https://code.claude.com/docs/en/model-config)、[出力スタイル](https://code.claude.com/docs/en/output-styles)、[更新履歴](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)の 2.1.323〜2.1.327。hooks・permissions・sandboxing・statusline・advisor など関連ページ 51 本も確認した
- Codex: [設定](https://learn.chatgpt.com/docs/config-file/config-reference)、[スキーマ](https://learn.chatgpt.com/docs/config-schema.json)、[モデル](https://learn.chatgpt.com/docs/models)、[名前付きレイヤー](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles)、[権限](https://learn.chatgpt.com/docs/permissions)、[公開版](https://github.com/openai/codex/releases) 0.167.2〜0.169.1、[PR #60646](https://github.com/openai/codex/pull/60646)、rust-v0.169.1 (コミット `c376cf73`) のソース

公式スキーマは、タグ rust-v0.169.0 と rust-v0.169.1 のスキーマとバイト単位で一致した。

Claude Code は、設定索引の 249 件と見出しの 249 件が同じ集合だった。配列の値を説明する 4 件を除いた 245 件に、廃止を記録した `subagentStatusLine` を加えて 246 件を同梱する。Codex は、設定表の 334 キーとスキーマの 1,212 パスがすべてカタログにある。資料から消えた廃止の 2 件を加えて 1,269 件になった。

前回コミットしたカタログからの Codex の追加は 46 件ある。43 件は、公開版 0.168.0 (30 件) と 0.169.0 (13 件) でスキーマに加わった項目である。残りの 3 件は、設定表の権限の 2 件と、正規表記に書き直したキー (`notice."hide_gpt-5.1-codex-max_migration_prompt"`) である。

## 公式資料の主な変更

| 項目 | 変更 | 根拠 |
|---|---|---|
| Claude `subagentStatusLine` | 廃止 (改名・移動ではない) | 更新履歴 2.1.323 |
| Claude `sandbox.autoAllowBashIfSandboxed` | 既定が `false` になった | 設定の一覧、更新履歴 2.1.325 |
| Codex `auto_review.extra_policy`、`auto_review.experimental_policy_template` | 廃止。指定すると警告が出て無視される | PR #60646 (0.169.0) |
| Codex `model`・`review_model` | `gpt-6.2-astra`・`gpt-6.2-sol` が推奨モデルに加わった | モデル一覧 |
| Codex `tui.notification_method` | `osc777` が加わった | スキーマ、0.168.0 のリリースノート |
| Codex の新しい機能フラグなど 43 件 | 追加。機能フラグ 12 件のうち 10 件と `guardianv2` は開発中 (UnderDevelopment) | スキーマ、公開版のソース |

モデルの別名、推論レベル、出力スタイル、`web_search`・承認方針・サンドボックスの値に変更はなかった。

## 生成器の修正

`scripts/update_catalog.py` の取り漏れと誤りを直し、どれも `scripts/test_update_catalog.py` に回帰テストを加えた。

| 症状 | 原因 | 修正 |
|---|---|---|
| Codex のモデルの候補に表示名 (`GPT-6.2 Astra`) が入る | カードの `name` (表示名) を読んでいた | `slug` を読み、モデル ID の形でなければ生成を止める |
| スキーマにある値 (`osc777`、`mcp_servers.<id>.auth` の `ema_auth`) が候補から落ちる | 設定表の値でスキーマの列挙を置き換えていた | 両方を候補にし、スキーマが互換用とする旧値 (`guardian_subagent`) は除く |
| Claude の 25 件で、既定値に説明文の語 (`--settings`、`verbose` など) が出る | 「unset, so …」の後の最初のバッククォートを拾っていた | unset・none で始まる記述は「未指定」にする |
| 説明の中の例が候補になる (`availableModelsMatch` の `claude-opus-5`)。自由入力の判定を誤る (`timeFormat`・`worktree.baseRef`) | 値の箇条の説明文まで読んでいた | 各項目の先頭の値だけを候補にし、値で始まらない項目と任意名を含む値で自由入力を判定する |
| `otelHeadersHelper` が構造化された値になり、引用符なしでは保存できない | 型の判定で "executable" の中の "table" に一致した | 単語として照合する |
| 適用範囲がパスの「.」で切れる (`worktree.location` が `User, in ~/`)。廃止の項目の適用範囲が未記載になる | 最初の「.」で切っていた。廃止の項目には Scope の箇条がない | 文末で切り、Scope がなければ設定索引の表の列を使う |
| 最上位の `personality` が廃止 (編集不可) になる | 一部の値だけの注記 (`friendly` と `pragmatic` は効かない) で廃止と判定していた | その規則を外した。`none` にはまだ効果があるので非推奨になる |
| ドットを含む名前 (`hide_gpt-5.1-codex-max_migration_prompt`) が 3 つの階層に分かれる | 公式表は引用なしのドット区切りで、生成器もそのまま区切っていた | スキーマのパスから部品を逆引きし、正規表記で書き出す。複数に当てはまれば生成を止める |

設定表の JS の行は、単一・二重引用符とエスケープを JS の規則で読むようにした。読めない行を黙って落とさず、生成を止める。

## 廃止・非推奨の記録

公式の設定表とスキーマに書かれていない状態は、2 つの方法で取り込んだ。設計は codex (gpt-6.2-sol) にも相談し、併用する方針で一致した。

`data/confirmed-status.json` には、更新履歴・PR・関連ページ・公開版のソースで確かめた状態を書く。根拠の URL、効力の始まる版、確認した版を持ち、資料の本文より優先する。資料から消えたキーは、前回のカタログの定義を引き継ぐ。今回は次の 5 件を記録した。

| 項目 | 状態 | 根拠 |
|---|---|---|
| Claude `subagentStatusLine` | 廃止 | 更新履歴 2.1.323 |
| Codex `auto_review.extra_policy`、`auto_review.experimental_policy_template` | 廃止 | PR #60646 |
| Codex `profile` | 廃止。書かれていると起動時にエラーになる | config-advanced#profiles、`codex-rs/core/src/config/mod.rs` L3547-L3554 |
| Codex `profiles` | 廃止。同じ名前を `--profile` で指定すると起動時にエラーになる | config-advanced#profiles、`codex-rs/config/src/loader/mod.rs` L306-L327 |

Codex の機能フラグの段階は、rust-v0.169.1 のコミットに固定した `codex-rs/features/src/lib.rs` と `legacy.rs` から読む。Removed は廃止、Deprecated と旧名は非推奨とした。結果は廃止 49 件、非推奨 13 件で、資料の本文で判定済みだった 5 件を含む。公式スキーマの `features` のキーを説明できないとき、資料の本文と段階が食い違うときは、生成を止める。

詳細ペインには、これらの状態の理由と出典を表示する。

## 日本語の説明

- Claude Code: 公式資料と食い違っていた 23 件を直した。今回の資料の変更によるものが 6 件 (既定値の変更、管理者設定での効き方など)、以前からのものが 16 件 (true・false の向き、効く範囲、値の形など)、任意の改善が 1 件である
- Codex: 設定表にあって説明のなかった 135 件と、新しい項目 9 件の説明を足し、計 404 件になった。既存の 14 件を直した。`model_providers.<id>.base_url` が Vertex AI 向けの文になっていた、`tui.theme` を画面全体の配色として説明していた、廃止・非推奨になった項目 (`profile`・`profiles`・`auto_review.extra_policy`・`features.multi_agent`・`personality` など) を現役として説明していた、などである。キー割り当ての新しい操作の名前 17 件も足した
- 候補の説明: Claude Code に、サンドボックスの自動許可・相談用モデル・推論レベルの上限を足した。Codex には権限 (`read`・`write`・`deny`) と通知方式を足し、モデルの候補を今の一覧に合わせた。既存の分も含めて今日の資料と照合し、`verified_at` を 2026-11-18 にした

新しい機能フラグ 11 件と `guardianv2` の子 2 件は、公式の説明がないため登録していない。画面には階層名を表示する。

## アプリの修正

`src/catalog.rs` に `key_path`・`key_text` を加え、項目の作成・表示・検索・名前の参照で正規表記を使うようにした。ドットや引用符を含む名前は 1 つの名前のまま照合・保存され、詳細ペインでは引用符で囲んで表示する。検索は引用符なしの表記でも一致する。

コードの全体レビューで見つかった不具合のうち、再現できた 8 件を直した。どれも修正前に再現し、回帰テストは修正を外すと失敗することを確かめた。

| 不具合 | 修正 |
|---|---|
| `[[skills.config]]` などのテーブルの配列を開いて変えずに反映すると、インラインの配列に書き換わりコメントが消える | テーブルの配列も全体編集にする |
| 構造化された値の項目で、文字列 `"0"` が引用符なしで編集欄に出て、変えずに反映すると整数 `0` になる | 編集欄を現在の値の表記で始める |
| 改行が CR だけで届く貼り付け (iTerm2 など) で行がつながる | CR と CRLF を LF にそろえる |
| TOML の文字列を変えずに反映すると、引用符やエスケープの表記が変わり未保存の扱いになる | 同じ型・同じ値なら文書を変えない |
| テーブルの配列や子のテーブルを `d` で消すと、見出しのコメントが消える | 見出しの上と行末のコメントも文書末尾に残す |
| Windows で AltGr の文字 (`{`・`[`・`@`・`\` など) が入力できない | Ctrl と Alt が両方付いた文字は入力として扱う |
| 長い 1 行の貼り付けが文字数の 2 乗の時間になる (10 万文字で 8.31 秒) | 1 行ごとにまとめて挿入する (0.4 ミリ秒) |
| 秘密情報の値が、候補の一覧の「現在の値」に平文で出る | 表示を許可するまで `[hidden]` にする |

## 検証

`make catalog-test`、`make ci` (単体テスト 87 件、CLI 7 件、インストール 2 件)、Windows 向けの clippy、astro-sight の review と impact は成功した。テストは Rust に 13 件、生成器に 18 件を加えた。

一時ディレクトリのプロファイルで、引用されたキーの切り替えと保存、権限の候補の選択と保存、廃止の項目の削除を確かめ、テストに含めた。依存の更新は depup で候補がなかった。間接依存 26 件の新しい版は、公開から 14 日未満か、14 日未満の版を必要とするため見送った。

## 構造の点検

astro-sight で、関数の複雑度と、編集・保存・一覧の組み立ての呼出し関係を確かめた。最も複雑なのはモーダルのキー操作 (`App::handle_modal`、複雑度 61) と通常のキー操作 (`App::handle_key`、33) で、前回と変わらない。どちらも操作のテストで分岐が押さえられており、今回の修正でも保守や検証を妨げる問題は出なかったので、分割は見送った。差分の review では、使われていない記号は無く、シグネチャを変えた生成器の `path_shape` の呼び出し元もすべて直っていた。

## 一覧生成の性能

キーの正規表記と状態の欄を入れた直後は、Codex の一覧生成が 13〜17% 遅くなった。部分的に元へ戻した版と比べると、多くはキーの分割・整形の分で、状態の欄の複製も一部を占めた。引用符を含まないキーは「.」で区切るだけで部品になる (検査した結果と同じになる) ので、分割と名前の参照にその近道を入れた。状態の理由と出典は、カタログの文字列を参照で持つようにした。

| 入力 | 変更前 | 近道を入れる前 | 変更後 |
|---|---:|---:|---:|
| Claude・追加 0 件 | 721 | 766 | 756 |
| Claude・追加 1,000 件 | 2,109 | 2,293 | 2,251 |
| Codex・追加 0 件 | 5,626 | 6,356 | 5,840 |
| Codex・追加 1,000 件 | 7,183 | 8,376 | 7,410 |

単位は µs の中央値。macOS 27.2.1 (arm64) の release ビルドで、同じカタログを使い、3 つのバイナリを交互に 4 回ずつ実行した (各回は準備 5 回・計測 25 回)。最終版は変更前の 1.03〜1.04 倍 (Codex) で、回ごとのばらつき (約 5%) の範囲に収まった。Claude の差も回ごとのばらつきの範囲である。性能の扱いは codex とも相談し、5% 以内に戻ればキャッシュを足さない方針とした。

カタログの更新の影響は、変更前のソースに前回と今回のデータを入れて比べた。Codex・追加 0 件で 5,415 µs と 5,437 µs で、差はなかった。前回の記録 (1,549 µs) より遅いのは計測時の環境の差で、日をまたいだ値は比べられない。生データと条件は[測定記録](maintenance-2026-11-18-benchmark.json)にある。

## 対象外と未確認

次の範囲は対象外とした。

- requirements.toml と、名前付きレイヤーの別ファイル
- 旧 `profiles.*` の子 (391 パス)
- main ブランチにだけある未公開のスキーマの追加分と、0.170・0.171 の alpha 版

確かめられなかった点は次のとおり。

- Claude Code 2.1.327 の `attributionProject` (資料に定義がない)、`tui` の値 `classic`、2.1.324 が公開されたか
- Codex の開発中の機能フラグをアカウントや実行環境で使えるか、`osc777` の動き、`tools.client_search_provider`・`tui.default_hosts` の値の意味、`guardian_extra_policy` の公式の記述
- `--profile` なしでは `[profiles.*]` が読まれないこと (ソースを読んだうえでの推定)

公式資料どうしの食い違いは、以前からのものを含めて残っている。`otel.exporter` の `statsig`、`tools.web_search` の真偽値、`computer_use.windows.always_allowed_app_ids`、`agents.max_threads` は、設定表とスキーマで扱いが違う。`auto_review.policy` は、設定表が既定の方針を「置き換える」、スキーマが「追加する」と書いている (説明は設定表に合わせた)。

レビューで未確定とし、修正しなかった指摘は次のとおり。

- ロックとバックアップの名前に使うハッシュ (`DefaultHasher`) が、Rust の版をまたいで同じ値になるとは限らない
- Windows で貼り付けがキー入力として届く場合の動き (実機では未確認)
- `profiles.toml` を保存し直すと、手で書いたコメントや `~` を使ったパスが消える
- TOML の構文エラーの列をバイト数で数えている (日本語の行では約 3 倍の値になる)
- ロックの失敗の理由にかかわらず、「別のai-config-tuiが保存中です」と表示する
- ヘルプ画面が 60 列×16 行でスクロールできない
- 1 回の描画やキー操作で、一覧を 2〜4 回作り直している
