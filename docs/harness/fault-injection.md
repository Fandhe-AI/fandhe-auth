# 障害注入のハーネス（TASK-13）

TASK-13 の設計の下書き。名前・形式の識別子・キー名・文言はすべて仮置きで、人が確認する。
記録は事実の出力で、合否の判定でも spec の「確定」でもない（ACPT-7）。stub の合格を実物の合格に数えない（A-2026-10-28-9）。
使い方は 8/8 で本書に足す。

## 置き場

`crates/it-harness` の module `fault`（`crates/it-harness/src/fault/`）。別の crate にしない（書き手の判断で、人が確認する）。
後続の段は `fault/` にファイルを足し、`mod.rs` に `mod` を 1 行足すだけにする。

## 注入の種類（1/8）

| 種類 | 名前 | 対象 | パラメータ | 担当の段 |
| --- | --- | --- | --- | --- |
| DbBlock | `db_block` | database | なし | 2/8 |
| DbDelay | `db_delay` | database | `delay_ms` | 2/8 |
| InstanceSigterm | `instance_sigterm` | instance | なし | 3/8 |
| InstanceSigkill | `instance_sigkill` | instance | なし | 3/8 |
| ExternalStoreBlock | `external_store_block` | external_store | なし | 4/8 |
| HubStubDelay | `hub_stub_delay` | hub_stub | `port`・`delay_ms` | 5/8（済） |
| HubStubRefusal | `hub_stub_refusal` | hub_stub | `port`・`refusal` | 5/8（済） |
| ClockSkew | `clock_skew` | instance（仮） | `skew_ms` | 6/8（済） |

根拠: AVLB-31（時計のずれ）・AVLB-33（`SIGTERM`・`SIGKILL`）・ORST-17（DB の外の保存先）。
パラメータは作るときに検査し、0 の遅れ・0 のずれを拒否する。遅れ・ずれの値は試験用の仮の値で要件の値ではない（THRT-29・A-2026-10-28-1）。
`ComponentKind` には種類を足さない（DB の外の保存先は構成要素ではない）。

## DB の遮断と遅延（2/8）

`crates/it-harness/src/fault/db.rs` の `DbFaultProxy`（仮称）。PostgreSQL の前に置くループバックの TCP 転送器で、
PoC-7 の手順 6 の向き（転送器が応答に遅延を足す・経路を遮断する）に合わせる。外部の crate は足さず `std::net` と `std::thread` だけで書く。

- 待ち受けも上流も `127.0.0.1` 固定。上流はポートだけを受け、任意のホストへは転送しない。TLS は素通しで、中身は読まない。
- 入口: `start_to_port`（試験用）・`start_for(EndpointHandle)`（`Database` の起動前に作れる。値が無い間は接続を閉じる）・
  `proxied_endpoint`（ポートを差し替えた接続情報。Host への結線は TASK-716 待ち）・`block`・`release_block`・`delay`・`release_delay`・`stop`。
- 遮断は開いている接続を切り、以後に転送器が accept した接続を直ちに閉じる。待ち受けは遮断中も閉じない。解除しても切れた接続は戻らないので、client はつなぎ直す。
- 遅延は応答（上流から client）の向きの塊ごとに足す。開いている接続にも効く。解除は同じ値で `Released` を記録する。
- 記録が成功してから状態を変える。基準線の `FaultTimeline` への注入は記録が拒否し、注入もされない。
  二重の開始は `AlreadyActive`、無い注入の解除は `NotActive`。
- 誤りと `Debug` にポート・アドレスを載せない（SECR-11・SECR-23）。
- 試験: `make test`（`tests/fault_db.rs`。Docker 不要）・`make it-db`（`tests/fault_db_docker.rs`。実際の PostgreSQL に SSLRequest を送る）。
  時間に依る確認は下限だけ。

## インスタンスの停止（3/8）

`crates/it-harness/src/fault/instance.rs` の `InstanceFaultHandle`（仮称。Unix 限定）。

- 入口: `spawn(&mut Command)`・`adopt(Child)`・`sigterm`・`sigkill`・`wait_exit`・`shutdown`。
- 対象はハーネスが所有する未回収の `Child` だけ。生の pid は受けない。回収した後は送らず `NotRunning`。
- `SIGKILL` は `Child::kill`。`SIGTERM` は固定パス `/bin/kill` をシェル・PATH なし・環境を空にして起動して送る。無ければ記録の前に `SignalUnavailable`。
- 記録してから送る。基準線では記録が拒否し、signal も送らない。記録の後に送信が失敗したら `SignalFailedAfterRecord`
  を返し、その記録は正しい記録として使わない。
- 停止の時刻は `Applied` の `at`・`offset_ms`（記録の形式は変えない）。`wait_exit` は終了の観察（signal 番号・終了コード）
  を返すだけで記録しない。注入の呼び出しは待たない。
- 再起動は `Released` にしない。呼び出し元が新しいハンドルを作る。再起動を回す試験（AVLB-33）は Hub `ARUN-76`。
- 誤りと `Debug` に pid・コマンド・`io::Error` の文言を載せない（SECR-11・SECR-23）。
- 試験: `make test`（`tests/fault_instance.rs`）。`SignalUnavailable` の経路は未検証。

## DB の外の保存先の遮断（4/8）

`crates/it-harness/src/fault/external_store.rs` の `ExternalStoreFaultProxy`（仮称）。

- 入口: `start_to_port`・`start_for`・`block`・`release_block`・`stop`・`listen_port`・`proxied_endpoint`。遅延は出さない。
- 遮断は TCP の経路の遮断。2/8 の転送器（`DbFaultProxy`）を `external_store_block` の種類で包む。開いている接続を切り、
  以後に転送器が accept した接続を直ちに閉じる（`refused_count` に数える）。待ち受けは遮断中も閉じず、ポートを持ち続ける。
  解除しても切れた接続は戻らない。
- 方式: accept のスレッドは、accept・遮断の状態の確認・切断用の複製の登録を `conns` のロックの下で一度に行う。上流への接続と
  中継はロックの外で行う（固まっても `block()` を待たせない）。`block()` は同じロックを取ってから `Applied` を記録し、通ったら
  続けて状態を変え、登録済みの接続をすべて切る（記録と実際の遮断の間に待ちを挟まない。受理した接続が遮断から漏れない）。
  確立中の接続は accept の時点の世代との差で見つけて切る。`release_block()` はロックを取り、`Released` を記録してから遮断を解く。
  記録が拒否されたら状態を変えない。待ち受けを作り直さないので、解除の失敗でポートを失うことはない。
- 境界の契約（ORST-17）。境界は「転送器が accept した時点の遮断の状態」:
  - 通常の accept での判定は、「accept したときに、`conns` のロックの下で観測した遮断の状態」で決まる。遮断状態なら、その接続は上流へ届かずに切られる。
    遮断の前に accept した接続でも、上流への接続の確立中に世代が変われば（遮断が入れば。解除が済んでいても）拒否される。
    `release_block()` は戻る前にロックを解放するので、呼び出しが戻る前でも通常の accept が通り得る（解除の完了の後に始まった接続は、使える上流があれば上流へ届く）。
    解除の呼び出しより前に待ち行列へ入っていた接続は、解除のときの排出（最大 4096 件）で切る。排出は `accept()` が `WouldBlock` を返した時点で終わり、その後で `Released` を記録して遮断を解くので、
    排出の終了から解除までの間にカーネルの待ち行列へ入った接続（解除と重なった接続）は、件数の上限に達していなくても解除の後に転送され得る（保証外・未定。上限の扱いは次節「遮断の順序と境界の契約」の `release_block()` の順序を参照）。
  - `block()` の時点で開いていた接続も切る。
  - 遮断中に TCP の接続が成立しても（connect は成功する）、遮断中に accept されれば往復は切断になる。
  - `release_block()` が `Ok(())` で戻った後に新しく始めた接続は、遮断では拒否されない。使える上流があれば上流へ届く（`Ok(())` は遮断の解除を意味し、転送の保証ではない。`EndpointHandle` が空のとき・上流への接続に失敗したときは、解除の後でも接続を閉じる。`DrainFailed`・記録の誤りで戻ったときは遮断が維持される。client は `Ok(())` を確かめてから再接続する）。解除の時点で待ち行列に残る接続の排出と、その上限は次節の契約に従う（ここでは繰り返さない）。
- 「遮断中に始めた接続は届かない」とは言わない。残る限界:
  - 解除の瞬間の待ち行列の接続と、上流への接続の確立中に遮断と重なった接続の扱いは、次節「遮断の順序と境界の契約」で決めた（#5983）。
  - 上流への接続（上限 5 秒）は accept のスレッドで同期に行う。固まっている間は後続の accept が止まるが、`block()` は待たされない。
- `refused_count`: 転送器が accept した後に閉じた接続を数える（遮断中に accept したもの・上流なし・上流への接続の失敗・確立中の遮断）。
  解除のときに待ち行列から排出して切った接続も数える。遮断中で待ち行列にあってまだ accept していない接続は、解除の排出までは数えない。
- 記録は `applied`・`released` の `external_store_block target=external_store`。基準線では記録が拒否し、注入もしない。
- 誤りと `Debug` にポート・アドレスを載せない（SECR-11・SECR-23）。
- 根拠: ORST-17・ORST-2。保存先の実体は未決（Hub `ASTORE-38`）。Host への結線は `TODO(TASK-716)`。
- 試験: `cargo test -p fandhe-auth-it-harness --test fault_external_store`（`make test` に含む）。
  - OS の条件（#6016）: `block_returns_promptly_while_accept_is_stalled_on_upstream` は、旧実装では待ち行列のあふれで SYN を捨てる挙動に依っており、OS ごとに結果が分かれた（過去の制約）。現行の試験は SYN の破棄に依らない。
    `ignore` で外さず全 OS で同じ契約を検証し、土台が組めない環境では観測した誤りの種類つきで失敗させる（AUDB-6。macOS での確認は未検証）。
    これに加えて、待ち行列のあふれに依る `block_returns_promptly_while_real_upstream_connect_is_stalled_linux` は Linux に限る。契約は変えず、停止点の外に残る OS の connect の 1 行（本物の connect だけをロックで囲む回帰）の検出を足すもの。
    現行の停止状態は、転送器が上流への接続の直前に置く同期点（`UpstreamStall`）で作る。試験は `wait_entered` で到達を待ってから `block()` を呼ぶ。
    宛先の決定と接続は転送器が行い、停止点は接続先も結果も変えない。待ちは解放・停止・上限 5 秒で解ける。
    使用中（生きている転送器に束縛済み）の停止点は別の転送器に渡せず、起動は `StartFailed` になる（1 つ目の観測を消さない）。転送器の停止・破棄の後は同じ停止点を再び渡せる（ORST-17・AUDB-6）。
    `upstream_attempt_count` は接続処理の直前に加算する診断用の回数で、到達の確認には使わない。
    `release_returns_in_time_under_reconnect_storm` は macOS で最後の往復が切れることが報告されており調査中。期待は緩めず、
    失敗の文言に失敗した段階・誤りの種類・集計値の差を出す。集計値は転送器全体の合計で、最後の接続の失敗箇所を断定しない。
  - 観測のスレッドの上流の accept には期限があり、期限内に受理されなければ観測不能として失敗にする（#6040。AUDB-6。macOS での確認は未検証）。

## 遮断の順序と境界の契約（#5983）

2/8 の `DbFaultProxy`（4/8 の `ExternalStoreFaultProxy` も同じ）の契約。根拠は ORST-17。保存先の実体は未決（ORST-2・Hub `ASTORE-38`）。

- `block()` の順序: `conns` のロックを取る → `Applied` を記録 → 遮断の状態を入れ、世代を進め、登録済みの接続を切る → ロックを外す。
  記録が拒否されたら状態を変えない（fail-closed）。先に状態を変えると「記録が無いのに切れた」が起こるので、この順を契約にする。
  `Applied` の `at`・`offset_ms` は実際の遮断の開始の下限で、時刻の関係は「`Applied` ≦ 実際の遮断の開始 ≦ `block()` が戻る時刻」。
- `block()` の保証の境界:
  - 呼ぶ前に終わった往復は影響を受けない。
  - 戻った後は、開いていた接続の上流向き・client 向きのどちらにも、転送器は新しい塊を書かない（カーネルの送信バッファに既に入っていた分は届き得る）。
  - 遮断中（`block()` が戻った後、解除が完了するまで）に転送器が accept した接続は、client のバイトを上流へ届けずに切る。判定はロックの下で観測した遮断の状態で、遮断の前に accept した接続も、確立中に世代が変われば拒否される。
  - 呼び出しと重なった往復は未定。試験はこの窓で合否を決めない。
- 確立中の接続: 上流への接続を始める前に遮断が入っていれば、上流へつながない。接続を試行してよい境界は「事前確認（遮断の状態と世代の確認）の時点」で、
  その後にポートを引いて `connect_timeout` を呼ぶまでの間に `block()` が完了しても、上流への TCP の接続の試行は
  起こり得る。ただし、client のバイトは 1 つも上流へ送らず、上流のバイトも client へ中継しない（`refused_count` に数える）。
  ロックを持ったまま上流へつなぐと `block()` が最大 5 秒待たされるので、試行そのものは防がない。
- `release_block()` の順序: ロックを取る → 待ち受けの待ち行列に残る接続を上限まで accept して直ちに切る（`refused_count` に数える）→
  `Released` を記録 → 遮断を解く。記録が拒否されたら遮断のまま戻る（排出は遮断中の accept と同じ扱いで、状態の変化ではない）。
  排出で `WouldBlock` 以外の accept の失敗（fd 枯渇等）があれば待ち行列の空を確かめられていないので、`Released` を記録せず遮断のまま `DrainFailed` で戻る（外部ストアの `release_block()` も `ExternalStoreFaultError::DrainFailed` で同じ）。`Interrupted` の再試行の上限（64 回）に達した場合も空を確かめられていないので同じ `DrainFailed` で、遮断を維持し `Released` を記録しない。再試行を使い切った場合は解除せず、`Released` も記録しない。件数の上限（4096）に達した場合は `DrainFailed` にせず解除へ進む（上限を超えた分は保証外）。この 2 つは区別する。
  ロックを持つ間は accept のスレッドが進めない。排出の後も、解除処理がロックを解放するまで進まないので、その間に新しく転送される接続は無い（解放の後に始まった接続は、使える上流があれば上流へ届く）。ただし排出の終了からロックの解放までの間に待ち行列へ入った接続は排出の対象外で、解除の後に転送され得る。
  時刻の関係は「`Released` ≦ 実際の解除 ≦ `release_block()` が戻る時刻」。
- `release_block()` の保証の境界: `Ok(())` を返した場合に限り、呼ぶ前に待ち行列に入っていた接続は上流へ届かずに切られる（件数の上限を超えた分を除く）。
  排出の終了から遮断の解除までの間に待ち行列へ入った接続（解除と重なった接続）は、上限に達していなくても解除の後に転送され得る（未定）。
  `Ok(())` は遮断を解除したことを示し、転送の保証ではない。`Ok(())` で戻った後に始めた接続は遮断では拒否されず、使える上流があれば上流へ届く。`EndpointHandle` が空のときや上流への接続に失敗したときは、解除の後でも閉じられる。`DrainFailed`・記録のエラーで戻った場合は遮断が維持され、新規接続も拒否される。
  client は `Ok(())` を確かめてから再接続し、`Err` のときは解除を呼び直すまでつなぎ直さない。呼び出しと重なって張られた接続は未定。
- 排出の上限は固定の開発上の上限で要件の値ではない（THRT-29）。仮の値は 4096。待ち行列の長さより大きいことは保証しない。割り込み（`Interrupted`）の再試行は件数とは別に 64 回で打ち切る。使い切ったら件数の上限とは違い保証外にはせず、`DrainFailed` で遮断を維持する（`Released` は記録しない）。再接続が続く状況でも解除が期限の中で戻るように件数で打ち切る。
- 遅延（`delay`・`release_delay`）は順序の契約の対象外。記録してから値を入れ、重なった塊は未定。
- 停止: accept のスレッドの終了後に共有の待ち受けを取り外す。中継の回収（join）を待たずに新規接続を受け付けない。
- 試験: `crates/it-harness/src/fault/db.rs` の単体（排出・排出の上限・確立中の遮断・`Released` の記録の時点で遮断中であること〔`released_is_recorded_while_still_blocked`〕）と `tests/fault_db.rs`
  （`block()` の後に上流へ届かない・client 向きが期限内に終わる・記録の時刻の下限・待ち行列の接続が上流へ届かない）。
  時間に依る確認は下限・期限だけにする。

## Hub の stub の遅れと拒否（5/8）

`crates/it-harness/src/fault/hub_stub.rs` の `HubStubFaultInjector`（仮称）。hub-stub の `HubFaultInjector`（TASK-7 #66）を
`FaultTimeline` と結ぶ。判定・内側を呼ばない約束（jti を既受信にしない・最終受信時刻を進めない・JWKS のキャッシュを変えない）は
hub-stub の実装をそのまま使い、作り直さない。

- 入口: `new(wait, &timeline)`（1 つの実行に結ぶ）・`finish`（注入器を消費し、記録と呼び出しの記録を組で返す。別の実行の記録は `RunMismatch`）・`apply`・`release`・`present`・`receive_logout`・`receive_heartbeat`・`serve_jwks`・`call_log`・`active_count`。
  受け口は呼び出しごとに借りる（`HubStubSession` に足さない）。種類は `hub_stub_delay`・`hub_stub_refusal`。
- 「TASK-7 の設定の仕組み」は `HubFaultConfig`・`FaultRule`・`CallWindow` と `set_faults` と読む。`HubStubConfig` には欄を足さない。
- 開始は `CallWindow { first: 1, last: None }`（以降すべて）の規則を入れた設定の差し替え、解除はその規則を除いた設定の差し替え。
- 同じ口の遅れと拒否（各最大 1 件）は 1 つの規則に合成する。hub-stub は最初に当たった 1 件だけを使うため、分けると片方が落ちる。
- 記録（`Applied`・`Released`）が先、設定の差し替えが後。基準線・記録の拒否では状態を変えない（fail-closed）。
- 事象に呼び出しの記録の件数（`log_pos`）を添える。呼び出し `i` は `log_pos <= i` の事象より後・次の事象より前に起きたと読み、同じ障害を二度適用・解除しても期間と呼び出しを順序で照合できる。
- 1 つの実行に結べる注入器は 1 つ（2 つ目の `new` は `AlreadyBound`）。`FaultTimeline` は `Clone` しない。公開の直接記録（`record_now`・`record_at`）は、結合の有無によらず Hub の stub の事象を常に拒否する（`HubStubEventOutsideInjector`）。注入器を結んだ実行の `FaultTimeline::finish` も拒否し（`InjectorBound`）、確定は `HubStubFaultInjector::finish` だけが行う。`log_pos` を付ける入口は注入器の内部だけ（`pub(crate)`）で、Hub の stub の事象は `log_pos` を必須にし（無ければ `LogPosRequired`）、Hub の stub 以外は受けず、位置の逆行は拒否する。形式の試験が要る人工の事象は、crate 内の単体試験（`#[cfg(test)]`）で組む。
- 注入器は実行をまたいで使わない。基準線・次の実行には新しい注入器を作る（有効な規則と呼び出しの記録を持ち越さない。fail-closed）。
- 解除は開始と同じ種類・値でだけ受ける。二重の開始は `AlreadyActive`、無い解除は `NotActive`。
- `MalformedBody` は JWKS の口だけ。`FaultKind::validate` が他の口を拒否する（1/8 の挙動を狭めた）。
- 遅れは `Wait` のフックへ渡すだけ。眠る実装は出さない。
- hub-stub の呼び出しの記録（`at` は試験の論理時刻）は `FaultRecord` に入れず、`call_log()` で読む。対応は順序だけ。`finish` が記録と呼び出しの記録を組で返すので、終了後にも照合できる。事象行に `log_pos` を足したため、記録の形式の版を `/2` に上げた。
- 対象外の口: `AuthPort`・`ChangeFeedPort`・`AuthJwksPort`（hub-stub の設計で対象外）。
- 誤りと `Debug` に token・値を載せない（SECR-11・SECR-23〜27）。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。
- 根拠: TASK-13・AVLB-18・AVLB-22・RVPG-28・OCL-36・Hub AUTHC-21。Host への結線は `TODO(TASK-716)`。
- 記録の例: `injection: 1 applied hub_stub_refusal target=hub_stub at=... offset_ms=... port=jwks refusal=status_503 log_pos=0`。
- 試験: `cargo test -p fandhe-auth-it-harness --test fault_hub_stub`（`make test` に含む）。

## 時計のずれ（6/8）

`crates/it-harness/src/fault/clock.rs` の `ClockSkewInjector`（制御側）と `SkewedClock`（読み取り側。仮称）。AVLB-31 の
「1 つのインスタンスの時計を段階的にずらす」操作の道具で、`CLM-20`・`OCL-26`・`SESS-18`・`SESS-19` の判定そのものは確かめない（ACPT-7）。
時計の差の測定と警報は Hub `AOPS-12`。

- 対象の決め方（仮）: 試験対象が読む壁時計の源（`SkewedClock`）に差を足す。OS の時計・DB のホストの時計・単調時計・子プロセスの時計はずらさない。
  LD_PRELOAD・環境変数・feature で切り替える経路は作らない（DPOL-30〜32）。
- 入口: `ClockSkewInjector::new`・`with_base_clock`（基準の時計を渡す。決定的な試験用）・`clock`・`apply`・`release`・`step`・`finish`・`abandon`・`active_skew_ms`。
  標準の終了手順は `ClockSkewInjector::finish(timeline, recorded_at)`（記録の確定と共有のずれの 0 への復帰を同時に行う。別の実行のタイムラインは `RunMismatch` で返し、何も変えない）。
  Hub の stub の注入器も結んだ実行は `HubStubFaultInjector::finish_with_clock(&mut clock_injector, timeline, recorded_at)` で確定する。時計の注入器を結んだ実行の `FaultTimeline::finish` と、
  どちらか片方だけの `finish` は拒否される（`ClockInjectorBound`・`InjectorBound`。AVLB-31）。`abandon` は記録を確定せず共有のずれだけを戻す用途（途中で実行を捨てる場合。以後その注入器の `apply` は `Abandoned`）。
  `SkewedClock` は `now`・`unix_seconds`（切り捨て。stub の `now: u64` 用）・`skew_ms`。
- 順序: 検査 → 二重の開始の確認 → 基準＋ずれが表せるかの確認（`OutOfRange`。何も記録しない）→ 記録 → 状態の変更。基準線・記録の拒否では状態を変えない（fail-closed。`step` の途中失敗は下記）。
- `step` は `Released(旧)` → `Applied(新)` の順に記録する。同じ値・0 は `InvalidParam`。`Released` の記録が拒否されたときは状態を変えない。`Released` が成功して `Applied` の記録だけが拒否されたときは、
  記録と一致させて「解除済み（ずれなし・適用中なし）」の状態で `Record` を返す（呼び出し側は誤りの後も旧いずれが有効とは見なさない）。
- 「記録の拒否で状態を変えない」は `apply`・`release` と `step` の `Released` の拒否に当たる。`step` の途中失敗（`Applied` の拒否）は上のとおり解除済みへ進む。
- 1 つの実行（`FaultTimeline`）に時計の注入器は 1 つ。同じ実行へ 2 つ目が適用すると `AlreadyBound`。時計の注入器を結んだ実行では、公開の `record_now`・`record_at` で時計の事象を積めない（`ClockSkewEventOutsideInjector`）。
  直接記録した時計の事象がある実行へは注入器を結べない。
- 解除は開始と同じ値で記録する。二重の開始は `AlreadyActive`、無い解除は `NotActive`。`Drop`・`abandon` は記録に触らず、配布済みの `SkewedClock` が共有するずれだけを 0 へ戻す
  （実行単位で試験条件を保つ。基準線の実行へ持ち越さない。AVLB-31）。解除し忘れは `Released` が無いので `unreleased_count` に出る。
- 記録の例: `injection: 1 applied clock_skew target=instance at=... offset_ms=... skew_ms=+1500`。記録の時刻は基準の時計のまま。
- 誤り・`Debug` にずれの値・時刻を載せない（SECR-11・SECR-23）。ずれの上限は置かない（THRT-29・A-2026-10-28-1）。
- Host への結線は `TODO(TASK-716)`。
- 試験: `cargo test -p fandhe-auth-it-harness --test fault_clock`（`make test` に含む）。

## 記録の形式（`fandhe-auth-it-fault-record/2`）

`RunRecord`（`fandhe-auth-it-run-record/1`）とは別のファイル。`run_started_at` で結ぶ。UTF-8・LF・1 行 1 項目の `key: value`。
注入した実行（`run_mode: fault`）と基準線（`run_mode: baseline`）で同じキーを出す。基準線は `injection_count: 0`・`unreleased_count: 0`。

```
format: fandhe-auth-it-fault-record/2
run_mode: baseline | fault
run_started_at: <RFC 3339 UTC>
recorded_at: <RFC 3339 UTC>
injection_count: <N>
unreleased_count: <M>
injection: <seq> <applied|released> <category> target=<target> at=<UTC> offset_ms=<n>[ パラメータ][ log_pos=<n>]
note: ...
```

- 基準線に事象を積む・Fault の実行で 0 件のまま記録を作る・`Applied` の無い `Released`・`offset_ms` の逆行は誤り（fail-closed）。
- 解除しなかった `Applied` は `unreleased_count` に数える。`SIGTERM`・`SIGKILL` は解除が無い一回きりの事象として数えない。
- 記録は enum・数値・静的な名前だけで組む。ホスト名・ポート・パス・接続情報・実行時の文字列を持たない（SECR-11・SECR-23・SECR-26）。

## 時計

時刻はハーネスのプロセスの基準の時計で押す（壁時計 `at`〔秒〕と、実行開始からの単調時計の `offset_ms`）。
試験対象に注入したずれた時計（6/8 の `SkewedClock`）や stub の引数の `now` は使わない。ずらすのは試験対象が読む `SkewedClock` だけ。hub-stub の `InjectionLog.at` は記録に入れず、`call_log()` で別に読む（5/8。事象の `log_pos` と呼び出しの順序で対応）。

## 未決・人の確認待ち

- 置き場を it-harness の module にしたこと
- 型・種類・形式の識別子・キー名・文言・ファイル名の形（すべて仮置き）
- 記録を `RunRecord` と別のファイルにし、`run_started_at` で結ぶこと
- 時計のずれ（6/8）: 対象を instance の壁時計の源（`SkewedClock`）にしたこと・DB のホストの時計と単調時計をずらさないこと・子プロセスのインスタンスへ注入しないこと・
  記録にインスタンスの識別が無く複数同時にずらすと区別できないこと（入れるなら版を上げる）・`step` の記録の形と同値の拒否・型名と文言
- インスタンスの停止を in-process の Host（TASK-716 待ち）にどう当てるか（3/8 はプロセスの子だけ）
- DB の外の保存先の実体（ORST-2・Hub `ASTORE-38` の決定待ち）。4/8 では仮に TCP の経路の遮断にした。人の確認待ち
- console stub の注入（TASK-8）を TASK-13 の一覧に入れるか（spec の列挙には無い）
- Hub の stub の注入（5/8）: 窓を「以降すべて」に限ったこと・同じ口の遅れと拒否の合成・`validate` を狭めたこと（`MalformedBody` は JWKS だけ）・
  眠る `Wait` を出さないこと（出すなら上限の決定が要る）・stub の呼び出しの記録そのものを記録の形式に入れないこと（入れるなら版を上げる。事象行の `log_pos` は `/2` で足した）・型名と文言
- 記録のファイルを秘密値の走査の対象に入れること（`secret-values.md`）。本段は canary の試験で代える。出力先は追跡ツリーの外（`target/`）
- DB の遮断の形を「直ちに閉じる」にしたこと。「受けて黙る（blackhole）」を入れるか（2/8）
- 遅延を応答の向きの塊ごとに足す形（2/8）
- 開発の値（accept の刻み 5 ms・待ちの刻み 10 ms・上流への接続の上限 5 秒・遅延の上限 600 秒）（2/8）
- 上流への接続は accept のスレッドで同期に行う。固まると後続の accept が最大 5 秒止まる（上流は `127.0.0.1` で通常は即座に結果が出る。試験専用の制約）（2/8）
- 転送器を `Component` にせず `ComponentKind` を 4 種のままにしたこと。Host への結線は TASK-716 待ち（2/8）
- `SIGTERM` の送り方（固定パスの `/bin/kill` か、`rustix` 等の依存を足すか）（3/8）
- 終了の観察の時刻を記録に入れるか（形式の版上げが要る）（3/8）
- 再起動を `Released` にしないこと（3/8）
- Unix 以外を対象外にすること、終了を見る刻み 10 ms の値（3/8）
- 保存先の遮断を TCP の形にし、PoC-10 のディレクトリの権限を外す形を入れなかったこと（入れるなら記録の形式の版上げとあわせて決める）（4/8）
- 遮断の形を 2/8 と同じ「直ちに閉じる」にしたこと。blackhole を入れるか（4/8）
- 転送器のスレッドの名前が `it-db-fault-*` のままであること（4/8）
- 遮断の順序を「記録してから状態を変える」のまま残したこと、解除のときに待ち行列を排出すること、排出の上限 4096（開発の値）、
  確立中に重なったとき上流への TCP の試行を許すこと（#5983）
- 世代の保存先（設定ストア・秘密の保管庫）と台帳の保存先を 1 つの種類で表すこと（4/8）
