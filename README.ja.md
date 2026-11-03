# winproc-tui

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: Windows 11 x64](https://img.shields.io/badge/Platform-Windows%2011%20x64-0078D6?logo=windows&logoColor=white)](#動作環境)
[![Rust](https://img.shields.io/badge/Rust-2024%20edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)

言語: [English](README.md) | [日本語](README.ja.md)

`winproc-tui` は、Windows 11 向けの軽量なプロセスモニタリングツールです。Rust で実装されており、CPU 使用率とメモリ使用量を抑えながら、軽快に動作します。

システム全体とプロセスごとの複数のメトリクスを監視し、リソース使用量の推移を可視化して、プロセスの挙動をターミナル上で調査できます。アプリケーションのメモリ、ハンドル、GUI リソース、GPU メモリ、I/O、.NET ランタイムなどに関する各種メトリクスの変化を時系列で追跡できます。

必要なメトリクスを最大 16 個の Graph に並べ、A/B マーカーで正確に比較し、セッションを記録してあとから調査できます。汎用的なシステム管理ではなく、開発・検証中に選択したプロセスを素早く繰り返し調べることに特化しています。

![システムとプロセスのメトリクス、Graph Workspace、Samples、A/B 比較を表示した winproc-tui のメイン画面](assets/screenshots/main-screen.png)

_`memory-eater.exe` の `Private Bytes` をグラフで表示し、A 地点から B 地点までの変化量を確認している例です。_

## インストール

Windows 用の公式バイナリの公開先は、[TX230/winproc-tui Releases](https://github.com/TX230/winproc-tui/releases) のみです。WinGet と [TX230 Scoop Bucket](https://github.com/TX230/scoop-bucket) は、どちらも同じ Release バイナリをインストールします。これら以外の配布元から入手したバイナリは、公式ビルドではありません。

### WinGet

```powershell
winget install winproc-tui
winproc-tui
```

更新とアンインストール:

```powershell
winget upgrade winproc-tui
winget uninstall winproc-tui
```

パッケージ ID を厳密に指定する場合は、上記の各 `winget` コマンドにある `winproc-tui` を `--id TX230.winproc-tui -e` に置き換えます。

### Scoop

```powershell
scoop bucket add tx230 https://github.com/TX230/scoop-bucket
scoop install tx230/winproc-tui
winproc-tui
```

登録済み Bucket を更新してから、アプリを更新します。

```powershell
scoop update
scoop update winproc-tui
```

アンインストールには `scoop uninstall winproc-tui` を使用します。通常は保存済み設定が保持されます。設定も削除する場合は `scoop uninstall --purge winproc-tui` を使用してください。

## クイックスタート

### プロセスの変化を Graph で見る

1. `PROCESSES` で調べたいプロセスを選びます。
2. `Left` / `Right` で `PrivBytes` などのメトリクスの列を選びます。
3. `Space` を押すかメトリクスセルをダブルクリックして Graph を追加します。同じ操作をほかのメトリクスでも繰り返すと、最大 16 個の Graph を同じワークスペースで比較できます。

システムの MEM、GPU、CPU、ネットワーク / ディスクは、Tracking List に登録しなくても各パネルから直接 Graph に追加できます。

![12 個のメトリクスを 3 列で表示した Graph Workspace](assets/screenshots/main-screen-12slots.png)

_レイアウトを変更し、12 個の Graph を表示している例です。_

### 2 時点を比較する

Graph または Samples にフォーカスを移し、`Left` / `Right` でサンプルを選びます。開始点で `a`、終了点で `b` を押すと、値の差と経過時間を確認できます。`x` で比較を解除します。

### プロセスを追跡・記録する

1. Process または PID セルを選び、`Space` または `t`、あるいは右クリックメニューの **Track process** でプロセス名を現在の Tracking List に追加します。
2. `Ctrl+T` から保存済み Investigation Profile を開けます。`Ctrl+S` で現在の Profile を上書き保存し、現在の Tracking List を新しい Profile として保存するときは `Investigation > Save profile as…` を使います。
3. `Ctrl+R` を押し、保存先と `1s` / `2s` / `5s` / `10s` の記録間隔を選んで開始します。
4. もう一度 `Ctrl+R` を押し、`y` で記録を終了します。`Enter`、`Esc`、`n` では記録を継続します。
5. `Ctrl+L` から保存済みログを開き、あとから調査します。

記録には Tracking List へのプロセス名の登録が 1 件以上必要ですが、一致するプロセスがまだ起動していなくても開始できます。対象名は記録開始時に確定し、そのセッション中は変わりません。`Shift+T` の All processes / Tracked-only 切り替えは表示だけを変更するため、記録中も使用できます。

`Tab` / `Shift+Tab` でパネルを移動し、方向キーで行、列、サンプルを選びます。`F1` または `?` で全操作を確認でき、画面下部には、その場で使える主な操作が表示されます。

### ネットワークのアドレスとポートを調べる

1. **Investigation > Network endpoints** または `F3` で、システム全体のネットワーク一覧を開きます。
2. 必要に応じて Filter に入力して絞り込み、`Enter` で一覧に移動します。初期表示では、接続待ちの TCP ポートと使用中の UDP ポートを確認できます。
3. `↑` / `↓` で行を選んで `Enter` を押すと、そのポートを使用しているプロセスの Process Info が開きます。

一覧は画面を初めて開いたときに取得されます。最新の状態を調べるには、一覧で `r` を押して更新します。

### ファイルを開いているプロセスを探す

1. **Investigation > Find processes by file** または `F4` で、ファイル検索画面を開きます。
2. 画面を開くと、取得可能な開いているファイルの一覧を自動で取得します。Query にファイル名やパスの一部を入力すると、即座に絞り込まれます（複数語はAND条件）。`Enter` で結果一覧に移動し、`Ctrl+R` で一覧を再取得できます。
3. 検索結果を `↑` / `↓` で選んで `Enter` を押すと、そのプロセスの Process Info が Files タブで開きます。一致したファイルのハンドルやアクセス権などを確認できます。

検索結果には、アクセスできなかったプロセスなど、調査できなかった範囲も表示されます。結果が 0 件でも、そのファイルを開いているプロセスが存在しないとは限りません。

どちらの調査も Live と記録中に利用できます。別の画面に切り替えても結果と入力内容は保持されますが、ログには記録されず、Log view では利用できません。

### .NETメトリクスを収集する

**.NET 8 / 9 / 10には、アプリの初期化中にメトリクス収集を開始すると、そのアプリがハングする可能性があるランタイムの不具合があります。** 起動時の自動接続を避けるため、winproc-tuiは毎回、収集が無効な状態で起動します。

1. 対象アプリの初期化が完了したことを確認します。
2. PROCESSESで対象プロセスを選択し、`n`を押して収集を開始します。
3. もう一度`n`を押すと、そのプロセスの収集を停止します。

トラッキングリストへの登録は不要です。COLUMNSで.NET列を選択しても、表示が変わるだけで収集は始まりません。新しく起動したプロセスや再起動したプロセスは、改めて収集を開始してください。

**.NET Frameworkアプリは今回の問題の対象外です。** 対応するメトリクスは、開始操作なしでPDHを通じて従来どおり自動収集します。

手動で開始する場合も、初期化中の収集には同じ危険があります。この対策は自動接続を防ぐもので、ランタイムの不具合そのものを修正するものではありません。関連するランタイムの報告や収集の制約は[.NET収集仕様](docs/dotnet-metrics-collection.md)を参照してください。

![収集開始前：PowerShellプロセスにグレー背景のNが表示され、.NETメトリクスの各列はOFFになっている](assets/screenshots/dotnet-metrics-off.png)

_収集開始前は、.NET列にOFFが表示されます。_

![PID 35964のPowerShellで収集を開始した状態：Nがピンク背景になり数値が表示され、ほかのプロセスはOFFのまま](assets/screenshots/dotnet-metrics-on.png)

_nを押すと、選択したプロセスに測定値が表示されます。ほかのプロセスはOFFのままです。Nは開始要求を受け付けた時点でピンク背景に変わるため、最初の測定値が届く前にもピンクになります。_

## 主な機能

- **CSV 出力**: **File > Export Graphs to CSV** から、登録したすべての Graph を 1 つの CSV に出力できます。保持している履歴全体、表示中の時間範囲、A/B 間から出力範囲を選べます。
- **ライブモニタリング**: システムのメモリ負荷、GPU アダプター別の負荷とメモリ、ネットワーク / ディスク、CPU、プロセス別の詳細メトリクスを表示します。
- **プロセスツリー**: Processes をソート可能なフラット一覧と、プロセスの親子関係を示すツリー表示に切り替え、フィルターやサブツリーの折りたたみを利用できます。
- **Graph と A/B 比較**: 最大 16 個のメトリクスを共通の時間軸で Graph に表示し、自由に並べ替えられます。Graph と連動する Samples で測定値や 2 時点の差分を確認し、選択中の Graph の A/B 区間内の最小値・最大値・平均値も調べられます。
- **プロセス群の追跡**: アプリのプロセス名を登録すると、確認できた子・孫以降のプロセスも合計、履歴、記録の対象になります。子孫と同じ名前でも、無関係なプロセスは対象に含めません。
- **Investigation Profiles**: あとから起動するプロセスも調査できるよう、Tracking List に名前を付けて保存します。追跡対象のプロセスが終了したあとも、最後に取得したメトリクス値を保持します。
- **.NET メトリクス**: .NET 8 / 9 / 10はプロセスごとに収集を開始でき、.NET Frameworkの対応するメトリクスは自動収集します。起動時の注意事項と操作手順は[.NETメトリクスを収集する](#netメトリクスを収集する)を参照してください。
- **Process Info**: 選択したプロセスのメトリクス、実行ファイルとランタイムの情報、開いているファイル、DLL、環境変数、ネットワークのアドレスとポート、スレッド一覧と各スレッドに優先的に割り当てられる CPU を 1 つのダイアログから調査できます。Scheduling タブでは CPU の優先度と、プロセッサグループが 1 つの環境での CPU アフィニティを確認・変更し、その場で適用したり直前の値に戻したりできます。
- **ファイルを開いているプロセスの検索**: ファイル名やパスからプロセスを探し、開いているファイルの詳細を確認できます。操作手順は[ファイルを開いているプロセスを探す](#ファイルを開いているプロセスを探す)を参照してください。
- **ネットワークの調査**: システム全体の TCP/UDP のアドレス、ポート、それらを使用しているプロセスを確認できます。操作手順は[ネットワークのアドレスとポートを調べる](#ネットワークのアドレスとポートを調べる)を参照してください。
- **記録と Log view**: システムメトリクスに加え、Tracking List の登録名に一致するプロセスとその子孫を JSON Lines 形式で記録し、Log view の Processes、Graph、Samples、A/B 比較を使ってあとから調査できます。

前回終了時の Tracking List とアプリ全体の設定は、次回起動時に復元されます。起動時に保存済み Investigation Profile または空の Tracking List を選ぶこともできます。Graph は次回起動時に復元されず、フィルター入力と実行時のプロセス識別子も保存しません。

## 向いている用途

`winproc-tui` は次のような調査に適しています。

- プロセスのリソース使用量を把握し、最適化の余地を調べる。
- メモリやハンドルなどのリソースリークがないか調べる。
- `Private Bytes` と `Working Set - Private` の差から、大きなバッファを確保したまま使っていない可能性を調べる。
- ハンドル数の推移と Process Info の Files を確認し、ファイルの閉じ忘れがないか調べる。各ファイルハンドルのキャッシュ設定、Write-through、同期・非同期対応、アクセス権も確認できます。
- プロセスが読み込んでいる DLL のパスやバージョンを調べる。
- 特定のファイルを開いているプロセスを探し、ファイルの開き方やアクセス権を調べる。
- 特定の処理やコード変更の前後でリソース使用量を比較する。
- 対象プロセスを記録し、現象が発生した時点付近の履歴をあとから調べる。

## 記録と Log view

記録の基点となる名前は、記録開始時点の Tracking List に固定されます。確認できた子孫も対象になり、親が先に終了しても追跡を続けます。子孫も含めて対象が実行されていない場合でも、システムのメモリ、GPU アダプター別メトリクス、CPU 集計値、ネットワーク / ディスクは記録され、プロセス一覧だけが空になります。

Live でのデータ収集とサンプル履歴は、常に 1 秒間隔です。記録間隔を長くすると、取得できたサンプルが平均化されるため、ファイルサイズと Log view の読み込み負荷が減る一方、短時間のスパイクは平滑化されます。1 回の記録は最大 24 時間で、上限に達すると自動的に Live へ戻ります。

Log view では、ログに保存されたプロセスの最後の状態が表示されます。記録した内容を時系列に沿って再生する機能はありません。記録済みの履歴は、Graph、Samples、Process Info、A/B 比較を使って調査できます。記録と Log view は同時に使用できません。

メトリクスの定義、集約方法、記録フォーマットは [docs/metrics.md](docs/metrics.md) を参照してください。

## ローカルMCP連携

MCPクライアントには `winproc-tui mcp-bridge` を登録してください。クライアントが起動するブリッジプロセスが、MCPサーバーとして動作します。ブリッジと画面を表示する本体は、同じ `winproc-tui.exe` を別々のプロセスとして実行します。TUI、メトリクス、履歴、調査状態は本体が管理します。

最初の有効なツール呼び出しで、ブリッジは次のように接続します。

- **WinProc TUIが起動済みの場合：** そのプロセスに接続し、現在の調査状態を共有します。既存のウィンドウをそのまま使い、TUIを追加で起動しません。
- **WinProc TUIが未起動の場合：** Windows Terminalの新しいウィンドウに本体を起動してから接続します。起動と接続の待機は最大12秒です。自動起動にはWindows Terminal、`wt.exe` のアプリ実行エイリアス、保存済み設定でMCP接続が有効になっていることが必要です。

MCPの初期化やツール一覧の取得（`tools/list`）だけでは、TUIの起動や接続は行いません。起動済みの本体がMCP接続を受け付けられない場合は、別の本体を起動せず、エラーを返します。本体とブリッジは同じWindowsログオンセッション、同じ権限レベルで実行します。

```mermaid
flowchart TD
    Client["MCPクライアント"]
    Bridge["MCPサーバー／ブリッジプロセス<br/>winproc-tui.exe mcp-bridge"]
    subgraph Application["画面を表示する本体プロセス：winproc-tui.exe"]
        TUI["TUIと調査状態<br/>メトリクス・履歴・追跡・記録"]
    end
    Client <-->|"標準入出力（stdio）／MCP"| Bridge
    Bridge <-->|"ローカル名前付きパイプ／内部JSONプロトコル"| TUI
```

図は、既存の本体への接続、または本体の自動起動が完了した後の構成です。MCP通信にTCP/IPは使用しません。調査対象のファイルがネットワーク共有にある場合は、通常のWindowsの権限に従ってアクセスできます。

本体への接続中は、TUI画面の右上、**LIVE**・**REC**・**LOG** のすぐ左に黄色の **MCP** マークが表示されます。ツールを呼び出していない間も表示され、最後のクライアントが切断すると消えます。ウィンドウ幅が極端に狭い場合は、動作モードや更新遅延・表示停止の表示が優先されます。MCPクライアントが切断しても、本体と実行中の記録は継続します。

MCP接続は初期設定で有効です。Settingsの **Disable MCP connections** をオンにすると、サンプリングや記録を続けたままクライアントを切断し、外部からの接続を無効にできます。保存済み設定でMCPが無効な場合は、TUIを手動で起動して有効にしてください。起動時にプロファイル選択画面が表示された場合も、選択を終えてからツール呼び出しを再試行します。

通常のブリッジは結果を最大5秒待ち、最後のページを返す際に一時データを解放します。続きが必要な場合は、次の呼び出しを結果に含めます。非同期ジョブや一時データの寿命をクライアント側で管理する場合は、`winproc-tui mcp-bridge --raw` を使用できます。

`tools/list` で、メトリクスと履歴、プロセス・ファイル調査、追跡とプロファイル、記録、保存ログの独立した分析、.NET収集の操作、条件待機、グラフ操作とCSV出力の全29ツールを確認できます。指定したシステム・プロセスのメトリクスをグラフに追加し、登録済みグラフをまとめて指定ディレクトリへCSV出力できます。追加と出力を1回の呼び出しで行うこともできます。記録・ログ関連機能を使う前に、TUIで記録先を設定してください。確認が必要な操作はTUIに表示されます。[仕様書](docs/mcp-integration-specification.md) と [検証結果](docs/mcp-system-testing.md) も参照してください。

## 動作環境

- Windows 11 x64 のみ。Linux と macOS には対応していません。

通常の監視に管理者権限は不要です。保護されたプロセスでは、Process Info の一部や開いているファイルの一覧を取得できない場合があり、取得できない値は `--` で表示されます。

同じ Windows セッション内で起動できる `winproc-tui` は 1 つだけです。後から起動した `winproc-tui` は、起動元のターミナルの表示や保存済みのセッション設定を変更せずに終了します。

## ソースからビルドする

ビルドには Rust 1.95.0 以降、Rust 2024 edition のツールチェイン、Build Tools for Visual Studio 2026 の MSVC リンカーが必要です。[rustup](https://rustup.rs/) で Rust を導入してからビルドします。

```powershell
git clone https://github.com/TX230/winproc-tui.git
cd winproc-tui
cargo build --release
.\target\release\winproc-tui.exe
```

Rust 開発者は、crates.io で公開されたソースパッケージをインストールできます。Cargo が手元でビルドするため、GitHub Releases で公開されるビルド済み Windows バイナリとは別の導入方法です。

```powershell
cargo install winproc-tui --locked
```

手元のソースコードからインストールする場合は、次のコマンドを実行します。

```powershell
cargo install --path . --locked
```

## 詳細情報

- アプリ内で `F1` または `?` を押すと、キーボードとマウスの全操作を確認できます。
- `winproc-tui --help` で起動オプションを確認できます。
- [docs/metrics.md](docs/metrics.md): メトリクス、取得元、表示形式、記録ログ。
- [docs/architecture.md](docs/architecture.md): システム全体の構成と、機能別設計書へのリンク。

## バグ報告・要望

不具合報告と機能要望は [GitHub Issues](https://github.com/TX230/winproc-tui/issues) へお願いします。Issue は日本語と英語のどちらでも構いません。

セキュリティ脆弱性の疑いがある場合は、公開 Issue ではなく [SECURITY.md](SECURITY.md) の手順で非公開報告してください。

個人開発のプロジェクトのため、外部コントリビューターからの Pull Request は、事前に依頼したものを除き受け付けていません。Issue の作成や議論は、Pull Request の依頼を意味しません。詳細は [CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## ライセンス

MIT License。詳細は [LICENSE](LICENSE) を参照してください。
