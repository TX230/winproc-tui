# winproc-tui

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: Windows 11 x64](https://img.shields.io/badge/Platform-Windows%2011%20x64-0078D6?logo=windows&logoColor=white)](#動作環境)
[![Rust](https://img.shields.io/badge/Rust-2024%20edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)

言語: [English](README.md) | [日本語](README.ja.md)

`winproc-tui` は、**プロセスごとのリソース使用量を時系列で確認するための TUI プロセス監視ツール** です。
Windows アプリのメモリ、ハンドル、GUI リソース、GPU メモリ、I/O などの現在値と時間変化をターミナルで確認できます。最大 16 個の Graph、A/B 比較、ログ記録と保存ログ表示により、開発・検証時のリソース挙動を調査できます。
Process Explorer や System Informer のような網羅的なシステム調査ではなく、対象プロセスの変化を素早く追うことに特化しています。Rust/Ratatui で作られています。

![winproc-tui のメイン画面。システムとプロセスのメトリクス、5 枚の Graph、Samples、A/B 比較を表示している](assets/screenshots/main-screen.png)

_システムの状態とともにプロセスのプライベートメモリ増加を確認し、A/B マーカーで 2 時点を比較している例です。_

## クイックスタート

### 1. 起動する

WinGet からインストールして起動できます。

```powershell
winget install --id TX230.winproc-tui -e
winproc-tui
```

`winproc-tui` は、同じ Windows セッション内で 1 つだけ起動できます。すでに起動中の場合、2 つ目はターミナル表示やセッション設定を変更せず終了します。

画面上部にはシステム全体のメモリ、GPU アダプター別の負荷とメモリ、ネットワーク / ディスク、CPU 使用率が表示され、`PROCESSES` パネルには実行中のプロセスが並びます。`Tab` / `Shift+Tab` でパネルを移動します。MEM と GPU はそれぞれ独立した移動先です。方向キーで行やカラムを選択します。

MEM、GPU、CPU の Usage / Threads / Processes、NW/DISK の System Activity は、プロセス名を登録しなくても起動時から自動的に履歴を保持します。Tracking List はプロセス名だけを対象にします。MEM または GPU にフォーカスがあるときは、`m` / `g` で両者を直接切り替えられます。`Left` / `Right` では MEM の左右列、または GPU アダプターを切り替えます。

コンパクトな `CPU` パネルには、全体使用率とユーザー / カーネル内訳、P/E コアの周波数、システム全体の Threads と Processes を表示します。`Up` / `Down` で Usage、Threads、Processes、最下段の `[Per-core Usage (P/E)]` ボタンを選択します。Usage、Threads、Processes は Graph 対象です。Per-core ボタンにフォーカスして `Enter` を押すか、ボタンをクリックすると論理 CPU ごとの使用率をスクロール可能なダイアログに表示し、`Enter` または `Esc` で閉じます。

### 2. プロセスのメトリクスを Graph で見る

1. `PROCESSES` で調べたいプロセスを選びます。
2. `Left` / `Right` で確認したいメトリクスカラムを選びます。例えば、`PrivBytes` はプロセスが占有するコミット済みメモリです。
3. `Space` を押すかメトリクスのセルをダブルクリックすると、Graph Workspace に追加されます。同じ操作をもう一度行うと削除されます。
4. ほかのメトリクスでも同じ操作を行うと、最大 16 個の Graph を比較できます。一覧内を移動すると、操作中のカードが表示範囲へ追従します。

`Space` とダブルクリックは、どちらも選択中の Graph だけを追加 / 削除します。MEM、GPU、NW/DISK、CPU パネルの選択中メトリクスでも同じ操作を使えます。`PROCESSES` とコンパクトなシステムパネルでは、Graph 登録済みの値を緑、アクティブな Graph の値を太字で表示し、Graph スロット番号用の幅は確保しません。

### 3. 2 時点の差を比較する

Graph または Samples にフォーカスを移し、`Left` / `Right` でサンプルを選びます。比較開始点で `a`、終了点で `b` を押すと、A/B 間の値の差と経過時間が表示されます。`x` で比較を解除できます。

### 4. プロセスを追跡・記録する

1. `PROCESSES` で対象プロセスを選びます。名前の左に反転表示の `T` が付いていなければ、`t` でプロセス名を Tracking List に登録します。`t` は登録 / 解除の切り替えです。
2. 繰り返し使う対象は、`Ctrl+T` の Tracking Lists で名前を付けて保存できます。
3. 必要に応じて `Shift+T` を押し、All processes / Tracked-only を切り替えます。Tracked-only 表示は記録の必須条件ではありません。
4. `Ctrl+R` を押し、保存先を指定して記録を開始します。
5. もう一度 `Ctrl+R` を押し、`y` を押すと記録を終了してログを閉じます。`Enter`、`Esc`、`n` では記録を継続します。
6. `Ctrl+L` で保存済みログを選び、内容を確認します。

記録開始には、Tracking List へのプロセス名の登録が 1 件以上必要です。登録した名前に一致するプロセスが現在実行されていなくても記録は開始できます。MEM、GPU アダプター別メトリクス、CPU の集計値、System Activity は登録不要で各フレームに記録されます。論理 CPU ごとの使用率は記録されず、プロセス一覧は一致するプロセスが現れるまで空になります。
記録開始ダイアログには対象名の件数が表示され、その Tracking List がセッション終了まで固定されます。記録中は `t` と `Ctrl+T` を使用できません。表示だけを切り替える `Shift+T` の Tracked-only は引き続き使用できます。

Tracking Lists ダイアログでは、名前付きプロセスリストのロード、保存、名前変更、削除を行えます。`Empty (default)` は作業中リストだけを空にし、Tracked-only は変更しません。`Tracking List startup` は `Resume last`、`Choose list`、`Start empty` を選ぶ左寄せの枠付きラジオグループです。`Tab` でフォーカスし、`Left` / `Right` / `Space` で変更して、`Enter` または `Esc` でダイアログを閉じます。リストのロードによって除外対象の保持履歴が失われる場合は確認を表示します。追跡名を削除する際にこの確認が必要な場合は、`Enter` で削除し、`Esc` でキャンセルします。ダイアログ内の全操作は、実行中に `?` で確認できます。

起動時の扱いを `Choose list` に設定すると、起動画面で `Up` / `Down` を使って Tracking List を選びます。`Enter` で選択したリストを使って開始し、`Esc` で初回サンプルを取得せずに終了します。

プロセス、システムメトリクス、Samples の選択行で `Ctrl+C` を押すと、Issue や調査メモへ貼り付けやすいプレーンテキストをコピーできます。長時間の調査では `.log` ファイルを残し、`Ctrl+L` から再度開けます。

### まず覚えるキー

| キー                  | 動作                                |
| ------------------- | --------------------------------- |
| `Tab` / `Shift+Tab` | パネルを移動する。                         |
| 方向キー                | 行、カラム、サンプルを選択する。                  |
| `Space`             | 選択中メトリクスの Graph を追加 / 削除する。         |
| `t`                 | プロセス名を Tracking List に追加 / 削除する（Live のみ）。 |
| `Shift+T`           | All processes / Tracked-only を切り替える。  |
| `Ctrl+T`            | 名前付き Tracking Lists を開く（Live のみ）。       |
| `Ctrl+F`            | プロセス一覧を絞り込む。                     |
| `Ctrl+R`            | 記録を開始する / 停止確認を開く。                |
| `Ctrl+L`            | 保存済みログを開く。                       |
| `?`                 | 全キー操作を表示する。                      |
| `q` / `Esc`         | 画面を戻る、または終了確認を開く。                   |

## 主な機能

- **モニタ**: 2 ページのメモリ負荷、GPU アダプター別の GPU / Encode / Decode 負荷とメモリ、ネットワークとディスク、CPU パネル、`WS Shrbl` を含むプロセス別メトリクスを表示。ソート、列選択、フィルター、ジャンプ検索で対象を絞り込めます。
- **グラフ表示**: 選択した最大 16 個のメトリクスを、1 つの Samples インスペクタを備えた、順序付きでスクロール可能な Graph Workspace に表示し、比較に必要な直近の履歴を保持します。
- **追跡 (Tracking Lists)**: 関心のあるプロセス名を登録し、追跡中のものだけを表示できます。用途ごとのリストに名前を付けて保存・切り替えでき、起動時に前回の作業中リスト、保存済みリスト、空のリストから開始方法を選べます。プロセスが終了したあとも最後に取得した値が画面に残ります。MEM、GPU、平均 CPU 使用率、System Activity は登録不要で常に履歴を保持します。
- **ログ記録と Log view**: 追跡中のプロセス、MEM、GPU アダプター別メトリクス、平均 CPU 使用率、システム状態を JSON Lines ログとして保存し、あとから同じ Processes / Graph / Samples / A/B の画面構成で再調査できます。
- **A/B 比較**: 任意の 2 時点を A 点・B 点としてマークし、値の差分と経過時間を表示します。
- **プロセス調査**: 選択中プロセスのメトリクス、実行ファイル情報、現在開いているファイルを、レスポンシブなタブ式 Process Info ダイアログで確認できます。
- **操作支援**: `Ctrl+C` で選択行をクリップボードへコピーでき、マウスでの行選択やスクロールバー操作にも対応しています。

## こんなときに役立ちます

- アプリのメモリ使用量が継続的に増えていないか調べたい。
- 特定処理の前後でメモリやハンドル数がどれだけ変化したか確認したい。
- 現在開かれているファイルを確認し、クローズ漏れ調査の手がかりにしたい。
- バックグラウンドサービスを **長時間記録** し、現象が起きた付近を Log view で見直したい。
- リファクタの前後でリソース使用量を比較したい。

## PerfMon との使い分け

Windows 標準のパフォーマンス監視ツールである PerfMon は、多数のカウンターや Data Collector Sets を扱う用途に向いています。`winproc-tui` は対象を絞り、実行中のプロセスとメトリクスを直接選び、カウンター設定なしで直近履歴を確認し、正確な A/B 比較と記録済みセッションの再調査を行うことに特化しています。

任意のカウンター設定、リモート監視、システム全体の収集管理には PerfMon を使い、開発・検証中に特定プロセスの変化を素早く調べる場合は `winproc-tui` を使う、という使い分けを想定しています。

## 動作環境

- OS: Windows 11 x64

Windows 専用です。Linux / macOS など他のプラットフォームには対応していません。

通常の監視に管理者権限は不要です。ただし、保護されたプロセスでは一部のプロセス情報や Open files を取得できない場合があります。取得できない値は `--` などで表示されます。

## ビルド済みバイナリを使う

### WinGet でインストールする

```powershell
winget install --id TX230.winproc-tui -e
```

インストール後は、任意のディレクトリから `winproc-tui` で起動できます。更新とアンインストールは次のコマンドで行います。

```powershell
winget upgrade --id TX230.winproc-tui -e
winget uninstall --id TX230.winproc-tui -e
```

GitHub Release の公開後、対応する最新版が WinGet カタログへ反映されるまで時間がかかる場合があります。その間に `winget install` を実行すると、古いバージョンがインストールされることがあります。`winget show --id TX230.winproc-tui -e` でカタログ上のバージョンを確認し、最新の Release より古い場合は、反映を待つか GitHub Releases の zip を使用してください。TX230 Scoop Bucket は WinGet カタログの審査・反映を経由しません。Bucket マニフェストの更新後は、以下の `scoop update` でローカルの Bucket を更新すれば、WinGet の反映を待たずに最新版を利用できます。

### Scoop（TX230 Bucket）でインストールする

```powershell
scoop bucket add tx230 https://github.com/TX230/scoop-bucket
scoop install tx230/winproc-tui
```

インストール後は、任意のディレクトリから `winproc-tui` で起動できます。更新時は、最初に `scoop update` で登録済み Bucket のローカルマニフェストを更新し、その後 `scoop update winproc-tui` を実行します。`scoop update tx230/winproc-tui` だけでは、ローカルの TX230 Bucket が古い場合に最新バージョンを検出できません。更新とアンインストールは次のコマンドで行います。

```powershell
scoop update
scoop update winproc-tui
scoop uninstall winproc-tui
```

通常のアンインストールでは設定が保持されます。設定も削除する場合は `scoop uninstall --purge winproc-tui` を使用します。

TX230 Bucket は公式 GitHub Release の zip をダウンロードし、SHA256 を検証して `winproc-tui` コマンドを登録します。追加のランタイムは不要です。

### zip を展開して使う

[GitHub Releases](https://github.com/TX230/winproc-tui/releases) から入手します。
ダウンロードした zip を任意のフォルダに展開し、`winproc-tui.exe` を実行してください。追加のランタイムやインストーラは不要です。
Release zip には `winproc-tui.exe` と `LICENSE` だけを含めます。ドキュメントは GitHub で公開します。

公式のビルド済みバイナリは [TX230/winproc-tui Releases](https://github.com/TX230/winproc-tui/releases) からのみ公開します。WinGet パッケージと [TX230 Scoop Bucket](https://github.com/TX230/scoop-bucket) は、この Release のバイナリを使用します。第三者によるコピー、ミラー、改変リポジトリで配布されるバイナリは公式ビルドではありません。

Release から zip と対応する `.zip.sha256` ファイルをダウンロードします。zip の SHA256 ハッシュ値を計算するコマンドは以下のとおりです。

```powershell
Get-FileHash .\winproc-tui-X.Y.Z-windows-x64.zip -Algorithm SHA256
Get-Content .\winproc-tui-X.Y.Z-windows-x64.zip.sha256
```

`Get-FileHash` の `Hash` と `.zip.sha256` の先頭に記載されたハッシュ値が一致することを確認してください。

## ソースからビルドする

開発中のコードを試したい場合は、ソースからビルドできます。

### 1. Rust ツールチェインを用意する

Windows では [rustup](https://rustup.rs/) の利用を推奨します。ビルドには Rust 1.95.0 以降と Rust 2024 edition、MSVC リンカー（Build Tools for Visual Studio 2026 の C++ ツールチェイン）が必要です。

winget を使う場合:

```powershell
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.BuildTools -e --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet --wait --norestart"
```

導入確認:

```powershell
rustup --version
rustc --version
cargo --version
```

### 2. ビルドして実行する

```powershell
git clone https://github.com/TX230/winproc-tui.git
cd winproc-tui
cargo build --release
```

実行ファイルは `target\release\winproc-tui.exe` に生成されます。
リポジトリの Cargo 設定により、Windows x64 ビルドには Microsoft C ランタイムが静的リンクされます。
ビルド後は次のいずれかで起動できます。

```powershell
cargo run --release
# またはビルド済みバイナリを直接実行
.\target\release\winproc-tui.exe
```

### 3. コマンドとしてインストールする（任意）

`cargo install --path .` を実行しておくと、ユーザーごとの cargo bin ディレクトリ（既定では `%USERPROFILE%\.cargo\bin`）に `winproc-tui.exe` がインストールされます。このディレクトリは PATH に含まれているため、以降は任意の場所で `winproc-tui` と入力するだけで起動できます。

```powershell
cargo install --path .
winproc-tui
```

## 起動オプション

起動オプションは現時点では以下の2つのみです。


| オプション           | 説明          |
| --------------- | ----------- |
| `-h, --help`    | ヘルプを表示する。   |
| `-V, --version` | バージョンを表示する。 |


## 操作リファレンス

README には主要操作のみを掲載します。**実行中に** `?` **を押すと、現在割り当てられている全キーをヘルプダイアログで確認できます。**

`f` のような 1 文字キーは、フォーカス中のパネルによって動作が変わります。Footer には、その時点で使用できる主要操作が表示されます。下の表ではパネルごとの操作をまとめています。

### 基本


| キー                  | 動作                             |
| ------------------- | ------------------------------ |
| `?`                 | ヘルプの表示 / 非表示。                  |
| `q` / `Esc`         | 終了確認を開く（Log view 中は live 表示へ戻る）。 |
| `Tab` / `Shift+Tab` | フォーカス移動。                       |
| `Ctrl+C`            | フォーカス中パネルの選択行テキストをコピー。         |
| `Ctrl+L`            | ログ一覧を開く。                       |
| `Ctrl+T`            | Tracking Lists を開き、組み込みの空リストのロード、名前付きリストの管理、起動時の扱いを設定する（Live のみ）。 |
| `Ctrl+R`            | 記録を開始する、または停止確認を開く。            |
| `Ctrl+P`            | 表示更新の一時停止 / 再開。サンプリングと記録は継続する（Log view 中は使用不可）。 |
| `Ctrl+Wheel`        | Windows Terminal のズーム倍率を変更。     |

終了確認では `Enter` または `q` で終了し、`Esc` でキャンセルします。


### プロセス操作


| キー                  | 動作                                      |
| ------------------- | --------------------------------------- |
| `Ctrl+F`            | プロセス名でフィルタリングする。`Full Path` 列を選択しているときは実行ファイルパスも対象にする。 |
| `Ctrl+I` / `Ctrl+J` | プロセス名のインクリメンタル検索。                       |
| `Space`             | 選択中のグラフ化可能なプロセス、MEM、GPU、NW/DISK、CPU Usage メトリクスを Graph Workspace に追加 / 削除する。 |
| `s`                 | 選択カラムでソート（再押下で昇順 / 降順切替）。               |
| `c`                 | カラムピッカーを開く。                             |
| `Shift+Up/Down`     | 稼働中プロセス行を連続範囲で選択する。                     |
| `Ctrl+Up/Down`      | 複数選択を変えずにカーソルだけ移動する。                    |
| `Ctrl+Space`        | 現在の稼働中プロセス行を複数選択に追加 / 削除する。             |
| `Shift+Left/Right`  | 選択中のメトリクスカラムを左 / 右へ移動する。                |
| `w` / `Shift+W`     | 選択中カラムを 1 セル広げる / 狭める。                    |
| `t`                 | 選択プロセス名を Tracking List に追加 / 削除（Live のみ）。 |
| `Shift+T`           | Tracked-only 表示を切り替える。                    |
| `d` / `Delete`      | 選択した稼働中プロセス行の終了確認を開く。                  |
| `Enter`             | 選択中プロセスの Process Info を開く。              |
| `i`                 | System Info ダイアログを開く。 |
| `f`                 | 選択中の稼働プロセスの Process Info を Files タブで開く。 |
| `g`                 | 設定済みの全 Graph を一括で開く / 閉じる。              |

Process Info には `Metrics`、`Image`、`Files`、`DLLs`、`Environment` の各タブがあります。ダイアログを再度開くと、前回表示していたタブにフォーカスが置かれます。操作対象のない `Metrics` と `Image` では、`Tab` / `Shift+Tab` を押してもタブからフォーカスを移動せず、`Up` / `Down` で本文をスクロールします。`Files`、`DLLs`、`Environment` では、`Tab` / `Shift+Tab` でタブと本文の間を移動できます。タブにフォーカスがあるときは `Left` / `Right` でタブを切り替えられ、`Ctrl+Left` / `Ctrl+Right` はどちらのフォーカスからでも使用できます。動的タブは `Ctrl+U` で更新し、選択中の値は `Ctrl+C` でコピーし、`Esc` でダイアログを閉じます。保護されたプロセス、非対応のプロセス、終了済みプロセスでは取得できない項目があります。

プロセス終了の確認では、`Enter` で選択した各ライブプロセスのPIDに対して個別に `taskkill /f /pid` を実行し、`Esc` でキャンセルします。同じイメージ名の別プロセスは対象になりません。このダイアログでは `y` と `n` を使用しません。

A/B 点が設定されている場合、`Metrics` は完全一致する時刻のサンプルを使って Current − A または B − A を表示します。環境変数には秘密情報が含まれる場合があります。値はダイアログを閉じると消去され、Recording や Log view には保存しません。


### Graph と A/B 比較


| キー                         | 動作                           |
| -------------------------- | ---------------------------- |
| `Enter`                    | 操作中のプロセス Graph の Process Info を開く。 |
| `Up`                       | 前の Graph スロットを選択。             |
| `Down`                     | 次の Graph スロットを選択。             |
| `Delete`                   | 操作中の Graph を削除。                  |
| `Left`                     | 古いサンプルを選択。                    |
| `Right`                    | 新しいサンプルを選択。                  |
| `Ctrl+Left` / `Ctrl+Right` | 表示範囲を左右に移動。                  |
| 右ドラッグ / `Ctrl`+左ドラッグ      | マウスで表示範囲を左右に移動。              |
| `PageUp` / `PageDown`      | Graph フォーカスでは表示時間幅を変更。Samples フォーカスではページ単位で移動。 |
| タイトルの `[-]` / `[+]`          | マウスで共通の表示時間幅を広げる / 狭める。                    |
| `f`                        | 登録中の全 Graph のサンプルが収まる共通時間範囲へ切り替え。 |
| `z`                        | Y 軸下限を 0 固定 / 表示範囲の最小値に切り替え。 |
| `v`                        | Samples テーブルの表示 / 非表示を切り替え。    |
| `d`                        | Samples の Delta 列の表示 / 非表示を切り替え。 |
| `l`                        | Graph 配置を Auto / 1 列 / 2 列 / 3 列の順に切り替え。 |
| `a` / `b`                  | 選択サンプルを A 点 / B 点としてマーク。     |
| `Shift+A` / `Shift+B`      | A 点 / B 点へジャンプ。              |
| `x`                        | A/B 比較をクリア。                  |
| マウスホイール                   | Graph 行をスクロール。Samples 上ではサンプル行をスクロール。 |


Graph Workspace には最大 16 個のカードを順序付きで登録できます。`Up` / `Down`、カードのクリック、マウスホイール、スクロールバーで操作中の Graph を選び、`Delete` またはカードの `[x]` で削除します。タイトルの `[-]` / `[+]` とカードの `[x]` は、マウスを重ねると強調表示されます。Samples インスペクタは操作中の Graph に追従し、共通操作は Graph と Samples のどちらからでも使用できます。

複数 Graph は絶対時刻に基づく1つの表示時間範囲、カーソル、選択時刻、A/B 点を共有します。`Fit all` は、履歴の開始・終了時刻が異なる Graph も含め、登録中の全 Graph で最古から最新までのサンプルが収まる範囲を表示します。Y 軸とサンプルの有無は Graph ごとに独立しています。A/B 値、クリップボード出力、記録ログは正確な値を維持し、選択時刻と完全一致するサンプルがない場合は近傍値で補わず `--` を表示します。

## 記録と Log view

`Ctrl+R` で記録を開始するか、記録中なら停止確認を開きます。停止確認を表示している間も記録は継続し、`Enter`、`Esc`、`n` では継続、`y` では停止します。記録を開始するには Tracking List に 1 件以上の名前が必要です。ログは JSON Lines として保存されます（拡張子 `.log`）。各フレームには MEM、GPU アダプター別メトリクス、平均 CPU 使用率、System Activity などのシステム指標と、Tracking List に一致する実行中プロセスが記録されます。一致するプロセスがその時点で存在しない場合も、システム指標は記録され、プロセス一覧は一致するプロセスが現れるまで空になります。記録開始時に保存先パスの入力ダイアログが開き、セッション終了まで固定される Tracking List の対象件数も表示します。保存先にはログのファイル名まで指定する必要があり、ディレクトリだけでは記録を開始できません。存在しない親ディレクトリは自動作成します。パス入力は常にフォーカスされ、`Enter` で開始、`Esc` でキャンセル、`Ctrl+Space` でディレクトリ名を補完できます。記録中は `t` と `Ctrl+T` を拒否して案内を表示します。Tracked-only 表示だけを変える `Shift+T` は引き続き使用できます。ログの作成、書き込み、フラッシュに失敗した場合は記録を終了し、部分ログを残してエラーダイアログを表示します。終了時のフラッシュに失敗した場合はアプリの終了を取り消します。Log view 中は記録を開始できず、記録中は Log view を開けません。

`Ctrl+L` でログ一覧を開きます。前回の記録ディレクトリがあればそこ、なければカレントディレクトリの `*.log` のファイル名をコンパクトな一覧に表示します。`Dir` 行で検索中のディレクトリを確認できます。`Up` / `Down` でファイルを選び、`Enter` で開き、`d` で別ディレクトリを指定し、`r` で更新し、`Esc` で閉じます。全キーバインドはダイアログ最下段に表示します。選択したログを開くと表示が `LOG` に切り替わり、Processes / Graph / Samples / A/B 比較で過去のセッションを調査できます。
Log view は再生機能ではありません。Processes は記録の最終値を表示し続け、Graph、Samples、Process Info で記録済みメトリクスの履歴を確認します。Process Info の静的情報には記録済み項目を使い、記録されていない項目は `--` と表示します。`Esc` で Live 表示へ戻ります。

記録ログのフォーマットと各フィールドの意味は [docs/metrics.md](docs/metrics.md) を参照してください。

## 設定の保存

レイアウト、表示カラム、ソート、Tracking Lists などのセッション設定は、次回起動時に復元されます。フィルター入力は保存しません。

## 開発者向けドキュメント

- [docs/metrics.md](docs/metrics.md): メトリクス、取得元、表示形式。
- [docs/architecture.md](docs/architecture.md): アーキテクチャ、実行時のデータフロー、設計判断、不変条件。

## 非目標

`winproc-tui` は次を目指しません。

- Process Explorer や System Informer の全面的な代替。
- 管理者権限を前提にした詳細取得。

短時間の開発・検証セッションで、プロセスの変化を素早く観察するためのツールです。

## バグ報告・要望

不具合報告と機能要望は GitHub Issues へお願いします。
バグ報告 / 機能要望それぞれのテンプレートを用意しています。

個人開発のプロジェクトのため、外部コントリビューターからの未依頼の Pull Request は受け付けていません。フィードバックや機能要望は Issue をご利用ください。

Issue は日本語・英語のどちらでも構いません。ユーザー向け README は日英の 2 言語で維持していますが、`docs/` 配下の詳細な仕様ドキュメントは英語のみで維持しています。

## ライセンス

MIT License。詳細は [LICENSE](LICENSE) を参照してください。
