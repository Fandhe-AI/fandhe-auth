---
description: >
  fandhe-auth（認証基盤）のセキュリティ規約。常に読み込む。OWASP Top 10 の観点は短く挙げ、認証基盤に固有の要求
  （CSRF Gate・Cookie・CSP とヘッダ・エスケープ・リダイレクト・転送ヘッダ・レート制限・トークン署名・本人確認の判定に
  LLM を通さないこと）を spec の ID 付きで定める。秘密情報を Git・ログ・PR・Issue に入れないこと、AI レビューの合格で
  安全と確定しないこと（CG-2・TASK-83）、問題を見つけたときの扱いも定める。security-auditor Agent はこれを基準にする。
---

# セキュリティ規約（fandhe-auth）

正本は `docs/spec/04-behavior/`（主に `csrf-ssr.md`・`api-session.md`・`token-signing.md`・`rate-limit.md`・`human-approval.md`）。
ここは実装・レビューのときに常に守る要点だけを持つ。秘密値の型と保存の形は `.claude/rules/secret-values.md`、
誤り応答の形と列挙防止は `.claude/rules/coding-rust.md`、依存の bans は `.claude/rules/dependency-policy.md`。

## OWASP Top 10 の観点（一般）

- A01 アクセス制御: サーバー側で拒否する。画面の表示・非表示を守りにしない（IRUI-21）。
- A02 暗号: 自前で実装しない（DPOL-47）。秘密値の保存は `.claude/rules/secret-values.md`。
- A03 インジェクション: SQL は `$n` のパラメータだけ（AUDB-43）。HTML は下の「エスケープ」。シェルへ外部入力を展開しない。
- A05 設定ミス: 検証を外す設定・feature を作らない（DPOL-30〜32・CSRF-7）。
- A06 既知の脆弱な部品: `cargo deny`・`cargo audit`（DPOL-34）。
- A07 認証・セッション: 下の「Cookie とセッション」「トークンの署名」。
- A09 ログ: 失敗は記録し、秘密値は出さない（SECR-23〜27）。
- A10 SSRF: 外向き HTTP は `reqwest` 1 つ・設定で渡した CA だけを信頼する・リダイレクトを追わない（DPOL-46）。

## CSRF とメソッド

- CSRF Gate を登録する箇所は 1 つ。Gate を外す feature・設定・環境変数は 0 件（CSRF-7）。拒否は 403・本文なし（CSRF-1）。
- 状態を変える操作を `GET` で受けない（405。CSRF-4）。
- ブラウザを通る経路の Gate の例外は `POST /authorize` の 1 パスだけ（完全一致。CSRF-14）。
- バックチャネルの除外は 1 パス 1 件の完全一致で列挙する。前方一致・ワイルドカード不可（CSRF-20）。

## Cookie とセッション

- 認証 Cookie は `__Host-` 接頭辞・`HttpOnly`・`Secure`・`SameSite=Lax`・`Path=/`・`Domain` なし（SESS-1）。
  他の Cookie も `__Host-`・`HttpOnly`・`Secure`・`Path=/`・`Domain` なしで、`SameSite` は項目ごとの原文に従う（CSRF-49・CSRF-50）。
- `Set-Cookie` を付ける箇所は発行と消去の 2 つだけ（CSRF-49・CSRF-50）。
- セッション値・トークンを `document.cookie`・`localStorage`・`sessionStorage`・ページのソースに置かない（SESS-3）。
- セッションに依る応答は `Cache-Control: no-store` と `Vary: Cookie`（CSRF-24）。refresh は不透明値（`D-2026-10-17-80`）。

## CSP とヘッダ

- CSP: `script-src 'self'`・`form-action 'self'`・`frame-ancestors 'none'` を全画面に（CSRF-28・CSRF-51）。
  `default-src 'none'`・`base-uri 'none'` も付ける（CSRF-32）。
- wasm を読む画面だけ `'wasm-unsafe-eval'` と `connect-src 'self'` を足す。他の画面に足さない（CSRF-29）。
- インラインスクリプトを書かない。`style-src 'unsafe-inline'` は builder の提供までの暫定で、外す方針（CSRF-30）。
- `X-Frame-Options: DENY`（CSRF-31）、`X-Content-Type-Options: nosniff`・`Referrer-Policy: no-referrer`（CSRF-32）。

## エスケープと誤りの画面

- 利用者の入力と攻撃者が選べる値（`redirect_uri`・`client_id` 等）は表示するなら HTML エスケープする（LGUI-14）。
- `redirect_uri` が登録と完全一致しない・`client_id` が未登録のときはリダイレクトせず、`400` の誤りの画面（OPRV-64・OPRV-71・LGUI-14）。
- 誤りの画面にスタックトレース・内部の状態を出さない（LGUI-14）。

## リダイレクトと接続元

- ログイン・ログアウトの戻り先は同一 origin のパスだけ。型を満たさない値は既定の遷移先に落とす。受け付けた値を再デコードして
  `Location` を作らない（CSRF-33〜35）。
- 転送ヘッダは許可リストのプロキシから来たときだけ、右から読んで許可リスト外の最初のアドレスを採る（CSRF-40・CSRF-41）。
- IPv6 の計数の単位は /64（CSRF-43）。レート制限は検証の前に判定し、判定と加算は 1 文（THRT-1・THRT-6・AUDB-27）。
- タイミング差の許容値は測って決める持ち越し（THRT-29）。推測で値を置かない。

## トークンの署名

- 出す `alg` は ES256 と RS256 だけ。`none`・HS 系を出さない（SIGN-1・SIGN-2・SIGN-8）。
- `typ` は ID トークン `JWT`・アクセストークン `at+jwt`・Logout Token `logout+jwt` を厳格に区別し、不一致は拒否（SIGN-9・SIGN-10）。
- 許可する `alg` をトークンのヘッダ・discovery・JWKS から導かない（SIGN-6）。`jku`・`x5u`・`jwk` で外から鍵を取らない（SIGN-16）。
- `exp == now` は拒否（SIGN-17）。

## 人の判定に LLM を通さない

- 本人確認・passkey の検証・復旧・権限付与・管理者承認の判定はプログラムが実行時に行い、LLM・検索を経由する経路 0（HAPR-1・SECR-28）。
- 人の確認は `bool` と方法の列挙で受け、自由文を判定に使わない（`human-approval.md`）。承認を AI・自動処理にさせない（04-behavior/README.md の (c)）。

## 秘密情報を外に出さない

- トークン・鍵・パスワード・復旧コード・接続文字列を Git・サンプル・試験の記録・ログ・PR・Issue・コメントに入れない（05-tasks.md の方針、SECR-23〜27）。
- コミット前の秘密情報の検出は `create-commit` スキルの手順に従う。検出器の出力に値そのものを出さない（件数だけで判定する）。
- 実機の名前・ホームのパス・内部のアドレスも書かない（04-behavior/README.md の (d)）。
- `git commit --no-verify`・`git push --no-verify` でフックを迂回しない。

## レビューの位置づけ

- AI のレビュー（security-auditor・ai-review 等）の合格だけで安全と確定しない（`D-2026-10-15-20`・CG-2）。人のセキュリティレビューは TASK-83。
- 「標準準拠済み」と書けるのは適合性テストで確かめた範囲だけ（ACPT-33）。〔仮想〕の合格で〔実機〕の項目を確定にしない（ACPT-7）。

## 問題を見つけたとき

| 重大度 | 例 | 扱い |
| --- | --- | --- |
| 高 | 秘密値の露出・CSRF Gate の欠け・検証を外す経路・`rsa` の混入 | 作業を止め、利用者に知らせて修正を待つ。PR を作らない |
| 中 | ヘッダの欠け・権限の過剰・例外の理由の欠け | 利用者に知らせ、修正を確かめてから続ける |
| 低 | 記録の不足・改善案 | レポートに書いて続ける |

未解決の高・中がある間は PR の作成・マージに進まない。例外として受け入れるかは人が決める。
