# AGENTS.md

`fandhe-auth` は Keycloak を使わない Rust 製の共通認証基盤の実装リポジトリ（public、`MIT OR Apache-2.0`）。
Hub が組み込むライブラリ（本体）で、型・判定・拡張契約・ストレージ契約・安全条件を持つ（`D-2026-10-30-1`・`LIB-1`）。
実行・運用（HTTP の受付・Gate の登録・Cookie の発行・DB の実装と migration・配送）は Hub の担当である。
本書は ai-review が PR の base コミットから読む **レビュー観点集**で、規約の本文は持たず、要点と根拠の所在を示す。
全体の運用は `CLAUDE.md`、規約の本文は `.claude/rules/` を正とする。

## 文書の位置づけとレビュアーへの注意

- 仕様は private の `Fandhe-AI/fandhe-auth-spec` を `docs/spec/` に submodule で置く。仕様の正は `docs/spec/04-behavior/`。
- **CI 上では private の spec submodule は取得されない。** spec の原文を確かめられないことを理由に指摘を作らない。
  spec に由来する指摘について判断するのは次の 2 点に限る。
  - 根拠の ID（`SECR-11`・`TASK-3`・`D-2026-10-29-1` 等）の有無と書式
  - `.claude/rules/` に書かれた要点との整合
- ルートの `Cargo.toml`・`crates/` は既にある（MS-1 の雛形）。「Rust の観点」は今の PR から適用する。
  文書・rules・scripts・workflow・Makefile・フックの観点も同じく適用する。
- 新しい基準はマージ後の次の PR から効く（ai-review は base コミットの本書を読む）。

## 重大度

| 区分 | 定義 | CI の扱い |
| --- | --- | --- |
| P0 | マージブロック。秘密値の露出、認証・認可・検証の迂回（署名検証を外す類型・`redirect_uri` 検証の欠けを含む）、`rsa` の混入、人の判定への LLM 経路、書き込み権限の過剰な付与・特権トリガー・新規の `self-hosted` runner の workflow など、直接の被害や取り返しのつかない不整合 | review ジョブが fail。ruleset で `codex / review` 等を必須チェックに登録しているとき、マージを止める。修正まで PR を進めない |
| P1 | マージ前に直す。規約違反・設計の整合崩れ・根拠の欠け・試験や検査の弱体化。ai-review の `block-priorities`（既定 `P0,P1`）により **P1 でも review ジョブが fail する。ruleset で必須チェックに登録しているとき、実質マージを止める** | review ジョブが fail。直すか、誤検知なら理由を書いて解消する |
| P2 | 提案。可読性・命名・軽微な重複・記録の不足・将来の改善案 | 止めない。指摘のみ |

- 観点 2〜4 で迷ったら重い側にせず、根拠（ファイル・行・ID・rules のパス）を示して区分する。根拠を示せない指摘は P2 以下にする。
- **観点 1（セキュリティ）には上の一般則を一切適用しない。** 迷ったら上位に区分し、「要確認」と書いて人の判断に回す。
  根拠（ID）を示せない場合も P2 に落とさず、「要確認」とする。
- **例外: 試験専用・開発専用の対象には次の物差しを観点 1 にも適用する**（上の「一切適用しない」の例外。本番の経路を持つ対象には適用しない）。
  対象は、試験専用の crate（`crates/*-harness`・`crates/*-stub`。本番の配布物に入らず、本番の経路を持たないもの）と、
  開発用のスクリプト（`scripts/` のうち CI・フックの判定に使わないもの）。
  - 既存の観点 1 の表で P0/P1 と定める違反（秘密値の直書き・コミットを含む）と、規約本文（`.claude/rules/security.md`・`.claude/rules/secret-values.md` 等）の要求に反するもの
    （例: シェルへ外部入力を展開する）は、試験専用・開発専用の対象でも従来の重大度判定（表と本文）のとおりとし、この例外より優先する。
    P2 以下に下げるのは、既存の要件（表と規約本文）を満たしたうえでの追加の堅牢化に限る。
  - 次は従来どおり P0/P1 とする: 実際の秘密値（トークン・鍵・パスワード・接続文字列・セッション ID 等）がログ・`Debug`・出力に出る、
    またはコード・試験データに直書きされる、本番の依存に入る（`Cargo.toml` の依存の向き・本番 crate への混入）、CI・workflow の権限やトリガーに関わる、
    検出・検査を弱める（秘密情報の検査の見落とし等）。
  - それ以外の堅牢化（プロセス管理・タイムアウト・境界の競合・規約が求める検査を満たしたうえでの、さらなる入力検査の厳密化・試験環境の上の攻撃者を仮定した対策）は、
    根拠（ファイル・行・ID）を添えて P2 以下にする。
    P2 以下にできるのは、既存の要件（表と規約本文）を満たしていることを確かめた上での追加の堅牢化に限る。
    抵触し得る規約（ファイル・ID）を具体的に挙げられ、適合しているか確かめられない場合は、従来どおり「要確認」として人に回す。
    抵触し得る規約を挙げられない堅牢化の提案は、P2 以下とする。
- 例外を受け入れるか、規約を緩めるかは人が決める。AI のレビュー合格は安全の確定ではない（CG-2・TASK-83）。

## 観点 1: セキュリティ

根拠の本文は `.claude/rules/security.md`・`secret-values.md`・`dependency-policy.md`・`github-actions.md`。

本体の要求値・契約・判定・試験は、下の表をそのまま適用する（要求値は緩めない。`D-2026-10-30-1` (1)(シ)）。
根拠に「実行は Hub」と付した ID は、要求値と判定の規則が本体、実行（登録・付与・発行）が Hub である。
本体の PR では要求値・契約・試験・組込み先へ渡す設定の型を見る。Hub の ID は値を写さず ID だけで引く。

| 区分 | 項目 | 根拠 |
| --- | --- | --- |
| P0 | トークン・鍵・パスワード・復旧コード・接続文字列をコード・サンプル・試験記録・ログ・コメント・PR・Issue に入れる | SECR-23・24・26・27（`SECR-25` は → Hub `ARUN-78`）・`security.md` |
| P0 | URL のクエリに秘密値を載せる設計を作る（`POST`・ヘッダ・一度だけの短期限の経路を使う） | `SECR-25`（→ Hub `ARUN-78`。本体の規則から外れたため ID だけで引く）・`secret-values.md` |
| P0 | 実機の名前・ホームのパス・内部のアドレス・個人情報の混入（文書・rules・コメントを含む） | 04-behavior/README.md の (d)・`security.md` |
| P0 | 本人確認・passkey の検証・復旧・権限付与・管理者承認の判定に LLM・検索を通す経路 | HAPR-1・SECR-28 |
| P0 | 検証を外す feature・設定・環境変数・要求ヘッダ（`no_gate`・`unchecked` 等）、試験専用のログイン分岐 | DPOL-28・30〜32・CSRF-7・`coding-rust.md` |
| P0 | `rsa`・`openssl` の直接宣言・`vendored`・`native-tls` を本番の依存に入れる。`openssl` の逆依存の根が `webauthn-rs` 系以外、配備先の OpenSSL への動的リンクでない構成。`jsonwebtoken` の `rust_crypto` feature | DPOL-10・11・13（`DPOL-12` は → Hub `AOPS-2`）・17・18・21・46 |
| P0 | 暗号・署名・乱数・パスワードハッシュ・WebAuthn 検証の自前実装 | DPOL-4・DPOL-47 |
| P0 | SQL に値を文字列で埋め込む（`$n` のパラメータ以外）。`format!` での SQL 組み立て | AUDB-43（→ Hub `ASTORE-91`。本体は SQL の実装を持たず、試験用の参照実装と契約に限る）・`db-layer.md` |
| P0 | workflow で `pull_request_target` 等の特権トリガーを PR のコードと組み合わせる、外部入力（PR タイトル・ブランチ名等）を `run:` に直接展開する | `github-actions.md` |
| P0 | 状態を変える操作を `GET` で受ける。CSRF Gate を外す・複数箇所で登録する | CSRF-1・CSRF-4・CSRF-7（本体は判定と例外の規則。Gate の登録の実行は Hub `ARUN-39`） |
| P1 | 認証 Cookie が `__Host-`・`HttpOnly`・`Secure`・`SameSite=Lax`・`Path=/`・`Domain` なしでない。`Set-Cookie` を発行と消去の 2 箇所以外で付ける | SESS-1・CSRF-49・CSRF-50（本体は属性の規則。発行の実行は Hub `ARUN-36`・`ARUN-38`） |
| P1 | セッション値・トークンを `document.cookie`・`localStorage`・`sessionStorage` に置く。セッション依存の応答に `no-store` と `Vary: Cookie` が無い | SESS-3・CSRF-24（本体は要求値。付与の実行は Hub `ARUN-36`・`ARUN-48`） |
| P1 | CSP・`X-Frame-Options`・`X-Content-Type-Options`・`Referrer-Policy` の欠け、インラインスクリプト、wasm 以外への `'wasm-unsafe-eval'` | CSRF-28〜32・CSRF-51（本体は要求値で正本は `ui-safety.md`。付与の実行は Hub `ASCR-3`・`ASCR-4`） |
| P0 | 署名検証を外す類型: `none`・HS 系の `alg` を受け入れる、`typ` の不一致を通す、許可する `alg` をトークンのヘッダ・discovery・JWKS から導く、`jku`・`x5u`・`jwk` で外から鍵を取る | SIGN-1・2・6・8〜10・16 |
| P0 | `exp == now` のトークンを通す | SIGN-17 |
| P0 | `redirect_uri` を登録との完全一致で検証しない、未登録 `client_id` へリダイレクトする（`400` の誤りの画面にしない） | OPRV-64・OPRV-71・LGUI-14 |
| P1 | 戻り先を同一 origin のパス以外から作る、誤りの画面への内部状態・未エスケープの値の出力 | CSRF-33〜35・LGUI-14 |
| P1 | 転送ヘッダを許可リスト外から信用する、レート制限を検証の後で判定する、判定と加算を 1 文にしない | THRT-1・THRT-6・AUDB-27、`CSRF-40・41`（→ Hub `ARUN-43`。接続元の取得は Hub） |
| P0 | 秘密値の型が `Debug`・`Display`・`Serialize` で値・値の一部・長さ・ハッシュを出す | SECR-11・12・17（検討中）・`secret-values.md` |
| P1 | 秘密値を生の `String`・`Vec<u8>` で持つ、`expose()` 以外の取り出し経路 | SECR-10・16・29〜33・`secret-values.md` |
| P1 | 外向き通信の口（`OutboundHttp`）が (1)〜(3) を満たさない: 設定で渡した CA 以外を信頼する（OS の信頼ストアを使う）、リダイレクトを追う、打ち切り時間が無い。失効の通知の送出の口も同じ (1)〜(3) を共有する | HOST-7 の (1)〜(3)・DPOL-46（契約は本体。実クライアントの選定は Hub `ARUN-14`） |
| P1 | Plugin に渡す外向き通信の口に、接続ごとの宛先の許可リストが無い、または許可リスト外・ループバック・リンクローカル・私設のアドレス・メタデータの宛先へ送る、discovery の応答で許可リストを拡張する。(4) と `HOST-8` の適用範囲は Plugin に渡す口だけで、失効の通知の送出の口には掛けない（宛先は登録済みと https で限り、宛先の IP 範囲を理由に拒否しない。RVPG-24・26・27） | HOST-7 の (4)・HOST-8・RVPG-24・26・27 |
| P1 | 本体の依存に HTTP クライアントの具象 crate（`reqwest` 等）を入れる（HOST-7 の期待欄は本体の依存で 0 件を要求し、実装は組込み先。HOST-9 は依存に現れてよいのを組込み先の実装側に限る） | HOST-7・HOST-9 |
| P1 | Plugin が `OutboundHttp` を介さず直接通信する（独自のクライアントの構築・ソケットを自前で開く） | HOST-9・PLG-22 |
| P1 | Plugin を本体と同じ依存監査・レビューの対象にしない。型・trait による「隔離」「サンドボックス」を能力として保証すると書く | PLG-22 |
| P0 | 本体のソースに発行者 `iss` の固定値を置く（`iss` は設定の型で受ける。同じ組み立ての中で応答・トークン間の `iss` が不一致） | DPOL-48 |
| P0 | メールアドレス（連絡先）の一致だけで外部 Identity を連結・統合する（検証済みの印の有無を問わない） | IDLK-4・LIFE-19 |
| P1 | OIDC Provider・RP 向け crate（`client`）・外部ログイン Plugin の 3 役割の型・設定を取り違えて渡せる、公開 API の文書の区別の表に載らない役割の型がある | LIB-19 |
| P0 | workflow で書き込み権限（`contents: write`・`pull-requests: write` 等）を過剰に付与する。public で新規に `runs-on: self-hosted` を書く（`ai-review.yml` の review ジョブの例外を除く） | `github-actions.md` |
| P1 | workflow の `permissions` の読み取りの範囲が必要より広い、アクションを SHA で固定していない | `github-actions.md` |
| P1 | checkout に `persist-credentials: false` が無い、ジョブに `timeout-minutes` が無い | `github-actions.md`（ジョブ追加時のチェックリスト） |
| P0 | `--no-verify`・`LEFTHOOK=0` などのフック迂回を促す変更。フックや CI の検査を黙って外す・緩める | `git-hooks.md`・`verification.md` |
| P1 | 例外（`ignore`・`skip`・許可の追加）に理由・期限・対象の範囲が無い。`cargo deny`・`cargo audit` を通さない依存の追加 | DPOL-20・34・35 |
| P2 | 失敗の記録の不足、ログ文言の改善、タイミング差の許容値を測らず推測で置く（持ち越し） | SECR-23・THRT-29 |

## 観点 2: アーキテクチャ・設計整合

根拠の本文は `CLAUDE.md`・`.claude/rules/coding-rust.md`・`db-layer.md`・`makefile.md`・`dotclaude-via-temp.md`。

| 区分 | 項目 | 根拠 |
| --- | --- | --- |
| P0 | `docs/spec/`（submodule）の中身を編集する。spec を直す PR をこの repo に混ぜる | `spec-reference.md` |
| P1 | `.agents/skills/`・`.claude/skills/`（`skills-lock.json` 管理の外部取得物）を直接編集する。`.claude/workflows/implement-issue-tree.js`（symlink）の実体を編集する | `CLAUDE.md` |
| P1 | 仕様の正を `docs/spec/04-behavior/` 以外に置く、実装が spec の ID と食い違う記述を規約として断定する | `spec-reference.md` |
| P1 | crate の配置・命名が `crates/<名前>`（crate 名は `fandhe-auth-` 接頭辞）に従わない。workspace を repo ルートの 1 つにしない。Hub の担当の crate（`db-pg`・`db-fandhe`・`db-migrate`・`server`）を本体の workspace に作る。段階 2 の crate（`backend`・`webauthn-wasm`・`frontend` 等）を利用者への確認なしに作る（段階は〔提案〕で、名前だけが決定済み） | D-2026-10-27-8・D-2026-10-28-1・DPOL-37・41・LIB-14 |
| P1 | `core` が非同期ランタイム・DB ドライバ・HTTP サーバー・`fandhe-backend`・`fandhe-frontend`・`web-sys` に依存する。`core` が現在時刻・乱数・ファイル・ネットワークを直接読む（時刻・乱数・ID は引数か注入で受ける） | DPOL-1・3 |
| P1 | `client`・`types`・`frontend` が `fandhe-auth-db*` に依存する。`types` に I/O 依存や `wasm32` で通らない依存を入れる。サーバーのビルドに `web-sys` を入れる（Hub のビルドの確認は `ARUN-16`） | DPOL-7・39・40 |
| P1 | DB 層: 汎用 CRUD の trait を作る、SQL・ロック・SQLSTATE を契約 trait や誤り型に出す、DB の現在時刻関数や `timestamptz` を使う、認証経路で「読んでから書く」を使う | AUDB-1〜3・16・36・`db-layer.md`（`AUDB-35` は → Hub `ASTORE-84`） |
| P0 | 契約テストの変異スイッチが本番ビルドで有効になる（既定で有効、環境変数だけで有効になる経路を含む。検証を外す経路に当たる） | DPOL-31・`coding-rust.md` |
| P0 | メモリの代替アダプターを有効にする feature が本番のビルドの依存解決に現れる、本番の実装の代わりに使われる経路がある | LIB-6・DPOL-31 |
| P1 | 契約テスト本体が PostgreSQL や `sqlx` に依存する。`Ops`（Hub の担当。`ASTORE-5`）を本体の workspace に置く、または本番の経路から到達させる | DPOL-37・38 |
| P1 | 誤り応答の形・列挙防止が rules と異なる（失敗理由を区別して返す等） | THRT-25・`coding-rust.md` |
| P1 | `.claude/` の変更が運用に従わない（`_/` に作ってから反映する経路を外れる）。Agent・Rule の frontmatter や参照パスが実体と合わない | `dotclaude-via-temp.md` |
| P1 | Makefile・`scripts/`・CI・フックで同じ検査を別実装にする。CI 独自のロジックを持つ。`make help` と `scripts/help.sh` の不一致 | `makefile.md`・`github-actions.md` |
| P1 | ワークフローを安易に増やす、集約ジョブ `ci-complete` の `needs` への追加漏れ、skipped を全許容する fail-open | `github-actions.md` |
| P2 | 責務の分け方・命名の改善案、`CLAUDE.md` と rules の記述の重複 | `CLAUDE.md` |

## 観点 3: 再利用・アセット化

根拠の本文は `.claude/rules/coding-rust.md`・`secret-values.md`・`code-comment-style.md`・`spec-reference.md`。

| 区分 | 項目 | 根拠 |
| --- | --- | --- |
| P1 | 他所有者（Hub・`fandhe-backend`・`fandhe-frontend`・`fandhe-db`）の値・本文を写す。ID・repo・commit で引くこと | `spec-reference.md`・`D-2026-10-15-21` |
| P1 | 相手側が未決の値を本番の値として推測で置く。設定で受け、試験用の仮の値で実装する | A-2026-10-28-1〜14 |
| P1 | 設定値・接続先・期限・上限をコードにハードコードする。環境変数で読むのは接続先・待ち受け・ログの置き場などの設定値だけで、読む変数の一覧を残す | DPOL-32 |
| P1 | 公開 crate（`client`・`secret`・`types` 等）の公開 API・設定の型に、内部の実装や秘密値の生の型が漏れる。非公開の型・bin crate に秘密値を持たせる | SECR-19・33・`secret-values.md` |
| P1 | 秘密値の型を `fandhe-auth-secret` 以外に新設する、PoC の実装（`Secret`・`Token`・`alt/`）や PoC の固定値（60 秒・300 秒等）を本番へ持ち込む | SECR-15・31・`coding-rust.md` |
| P2 | 汎用部（時刻・乱数・秘密値・検証部）を用途固有の crate から切り出せる、重複した実装、公開 API の rustdoc の不足 | `code-comment-style.md` |
| P2 | コメントが「何をしているか」だけで、根拠の ID（例: `// THRT-25: …`）や理由が無い。日本語を基本とする方針から外れる | `code-comment-style.md`・`japanese-style.md` |

## 観点 4: この repo 固有の規約

| 区分 | 項目 | 根拠 |
| --- | --- | --- |
| P1 | AI・自動処理が spec の振る舞いを「確定」にする記述。〔仮想〕〔モック〕の合格で〔実機〕〔人の操作〕の項目を確定扱いする。「標準準拠済み」を `ACPT-34` の確定の範囲外に書く | ACPT-7・ACPT-33・ACPT-34・`verification.md` |
| P1 | 試験の期待・検査・lint を緩めて通す（期待値の差し替え、`#[ignore]`、検査の削除、許可リストの拡大） | AUDB-6（`AUDB-69` は → Hub `ASTORE-2`）・`verification.md` |
| P1 | 完了の主張に証拠が無い（実行したコマンドと終了コードが PR に無い）。推測語（「〜のはず」等）での完了の宣言 | `verification.md` |
| P1 | PR 本文に対象の `TASK-N` と、実装・試験した振る舞い ID が無い。満たしていない期待が残りとして書かれていない | `spec-reference.md` |
| P1 | 追跡先（`TASK-N`・Issue 番号）の無い `TODO` を残す。スコープ外の修正を同じ PR に混ぜる | `out-of-scope-tracking.md`・`code-comment-style.md` |
| P1 | この repo は public で spec は private。spec 本文のまとまった転記を文書・コメント・PR・Issue に入れる | `spec-reference.md` |
| P1 | PR タイトル・コミット件名が Conventional Commits（`<type>[(<scope>)][!]: <要約>`）に従わない。type は `feat` `fix` `docs` `style` `refactor` `perf` `test` `build` `ci` `chore` `revert` | `conventional-commits.md`（正は `scripts/hooks/commit-msg-check.sh`） |
| P1 | 公開 crate の公開 API・設定の型・DB の列の形を変えるのに破壊的変更の印（`!` または `BREAKING CHANGE:`）が無い | `conventional-commits.md` |
| P2 | 要約が 72 文字を超える、末尾に句点がある、本文に背景・根拠の ID が無い | `conventional-commits.md` |
| P2 | 文体が常体でない、コード・識別子を翻訳している、絵文字の多用 | `japanese-style.md` |

## Rust の観点（crate を足す PR から適用）

`unsafe` 等のセキュリティに関わる項目は観点 1 と同じ扱いとし、迷ったら上位に区分して「要確認」と書く（上の一般則を適用しない）。

| 区分 | 項目 | 根拠 |
| --- | --- | --- |
| P1 | `unsafe` を使う（`#![forbid(unsafe_code)]` が既定。例外は手順に従い理由を書く） | `coding-rust.md` |
| P1 | `cargo fmt --check`・`cargo clippy --all-targets -- -D warnings`・`cargo test` を通さない。make ターゲットができた後は make を正とする | `verification.md` |
| P1 | `Cargo.lock` をコミットしない、版の固定が方針と異なる（本体は `Cargo.lock` のコミットと `webauthn-rs` 等の版の記録。相手側の部品の版固定は Hub の担当） | DPOL-4・DPOL-10・PKEY-4（`DPOL-9`・`CSRF-13` は → Hub `ARUN-17`） |
| P1 | 自身の crate に `license = "MIT OR Apache-2.0"` が無い。git 依存・crates.io 以外の registry | DPOL-25・26 |
| P1 | 未定の値（依存の版の固定の手順、`deny.toml` の中身・bans に足す名前〔TASK-668〕、`SECR-21` の構文木の走査の置き場〔`D-2026-11-20-10`〕など。`tracing`・`metrics` の feature の選びと K11 の判定は `D-2026-11-20-14` で決定済み）を決まったものとして書く（未定は「未定（TASK-n で決める）」と書く）。決定済みの MSRV 1.100・edition 2024（`D-2026-11-20-12`）と食い違う値を書く | `coding-rust.md`・`spec-reference.md` |
| P1 | 本番の依存の木に `tracing-subscriber`・`proptest`・`cargo-fuzz`・`libfuzzer-sys` などの試験の道具が入る（dev-dependencies か別の workspace に置く。`D-2026-11-20-7` の K7・`D-2026-11-20-8`・`D-2026-11-20-9`）。本体が受ける側（subscriber・exporter）を選ぶ（`D-2026-11-20-5`） | `coding-rust.md`・`dependency-policy.md` |
| P2 | 新しく書くログ・誤り応答の文言が英語でない（`D-2026-11-20-13`。外に返す誤り応答の形は THRT-1 等に従う） | `japanese-style.md` |

## 指摘の書き方

- 指摘には、該当のファイル・行、破られる規約の根拠（本書の表の ID または rules のパス）、直し方の方向を書く。
- 根拠の ID を spec の原文で確かめられない場合は、ID の書式と rules の記述との整合だけを見る（上記の注意）。
- 判定・値の決定・例外の承認・「確定」への変更は人が行う。レビューは判断材料を示すところまでとする。
