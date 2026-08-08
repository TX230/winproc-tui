# winproc-tui

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: Windows 11 x64](https://img.shields.io/badge/Platform-Windows%2011%20x64-0078D6?logo=windows&logoColor=white)](#動作環境)
[![Rust](https://img.shields.io/badge/Rust-2024%20edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)

言語: [English](README.md) | [日本語](README.ja.md)

`winproc-tui` は、**プロセスごとのリソース使用量を時系列で確認するための TUI プロセス監視ツール** です。
Windows アプリのメモリ、ハンドル、GUI リソース、GPU メモリ、I/O などの現在値と時間変化をターミナルで確認できます。最大 4 つの Graph、A/B 比較、ログ記録と保存ログ表示により、開発・検証時のリソース挙動を調査できます。
Process Explorer や System Informer のような網羅的なシステム調査ではなく、対象プロセスの変化を素早く追うことに特化しています。Rust/Ratatui で作られています。

![winproc-tui のメイン画面。プロセス一覧、GRAPH#1、Samples、A/B 比較を表示している](assets/screenshots/main-screen.png)

_追跡、表示の一時停止、A/B 比較を使用してプロセスのプライベートメモリを調査している例です。_

## クイックスタート

### 1. 起動する

WinGet からインストールして起動できます。

```powershell
winget install --id TX230.winproc-tui -e
winproc-tui
```

GitHub Release の公開直後は、WinGet カタログへの最新版の反映が遅れ、古いバージョンがインストールされる場合があります。TX230 Scoop Bucket は WinGet の審査・反映を経由しないため、Bucket 更新後に `scoop update` を実行すれば、WinGet の反映を待たずに最新版をインストールできます。Release 直後など、Bucket 更新前に最新版を使用する場合は、[GitHub Releases](https://github.com/TX230/winproc-tui/releases) から zip をダウンロードして展開し、`winproc-tui.exe` を実行してください。追加のランタイムは不要です。

画面上部にはシステム全体の RAM / VRAM、ネットワーク / ディスク、CPU 使用率が表示され、`PROCESSES` パネルには実行中のプロセスが並びます。`Tab` / `Shift+Tab` でパネルを移動し、方向キーで行やカラムを選択します。

RAM / VRAM、平均 CPU 使用率、NW/DISK の System Activity は、プロセス名を登録しなくても起動時から自動的に履歴を保持します。Tracked List はプロセス名だけを対象にします。

### 2. プロセスのメトリクスを Graph で見る

1. `PROCESSES` で調べたいプロセスを選びます。
2. `Left` / `Right` で確認したいメトリクスカラムを選びます。例えば、`Private` はプロセスのプライベートメモリ使用量です。
3. `1` を押すと、その値が `GRAPH#1` に表示されます。
4. 同様に `2` ～ `4` を使うと、最大 4 項目を並べて比較できます。

同じ番号をもう一度押すと、その Graph スロットを解除できます。`0` を押すと、すべての Graph を解除します。RAM / VRAM、NW/DISK、CPUS の各パネルでも、メトリクスを選んで `1` ～ `4` を押すと Graph に表示できます。

### 3. 2 時点の差を比較する

Graph または Samples にフォーカスを移し、`Left` / `Right` でサンプルを選びます。比較開始点で `a`、終了点で `b` を押すと、A/B 間の値の差と経過時間が表示されます。`x` で比較を解除できます。

### 4. プロセスを追跡・記録する

1. `PROCESSES` で対象プロセスを選びます。名前の左に反転表示の `T` が付いていなければ、`Space` でプロセス名を Tracked List に登録します。`Space` は登録 / 解除の切り替えです。
2. 繰り返し使う対象は、`Ctrl+T` の Tracked Lists で名前を付けて保存できます。
3. 必要に応じて `t` を押し、All processes / Tracked only を切り替えます。Tracked only 表示は記録の必須条件ではありません。
4. `Ctrl+R` を押し、保存先を指定して記録を開始します。
5. もう一度 `Ctrl+R` を押すと、記録を終了してログを閉じます。
6. `Ctrl+L` で保存済みログを選び、内容を確認します。

記録開始には、Tracked List へのプロセス名の登録が 1 件以上必要です。登録した名前に一致するプロセスが現在実行されていなくても記録は開始できます。RAM / VRAM、平均 CPU 使用率、System Activity は登録不要で各フレームに記録され、プロセス一覧は一致するプロセスが現れるまで空になります。

Tracked Lists ダイアログは、上段の「リストをロードする領域」と下段の「現在の Tracked List を保存する領域」に分かれています。上段の先頭には組み込みの `Empty (default)` が常に表示され、その下に保存済みの名前付きリストが並びます。行を選んで `Enter` を押すとロードでき、`Empty (default)` はクリックでも直接ロードできます。この項目をロードすると作業中の Tracked List だけが空になり、独立した Tracked only 設定は変わりません。古い保持履歴が破棄される場合は、名前付きリストと同じ確認が表示されます。アクティブな項目には `(*)` が付きます。`Empty (default)` がアクティブになるのは、名前付きリストがアクティブではなく、作業中リストが空の場合だけです。この組み込み項目は設定へ保存されず、`F2` での名前変更、`Delete` での削除、`Save` での上書きはできません。保存済みリストの各行の右側にはプロセス名が表示され、表示幅に収まらない場合は先頭の名前と残件数に省略されます。下段のリスト名には現在の名前付き Tracked List 名があらかじめ入力され、`Save` で現在追跡しているプロセスをその名前に保存します。新しい名前ならリストを作成し、既存の名前なら内容を更新します。保存結果は名前入力欄の直下に表示されます。`Tab` / `Shift+Tab` でリスト、名前入力、各ボタンへフォーカスを移せます。マウスをボタンへ重ねた場合も操作対象が強調表示されます。

### まず覚えるキー

| キー                  | 動作                                |
| ------------------- | --------------------------------- |
| `Tab` / `Shift+Tab` | パネルを移動する。                         |
| 方向キー                | 行、カラム、サンプルを選択する。                  |
| `1` ～ `4`           | 選択中のメトリクスを Graph に表示する。            |
| `Space`             | プロセス名を Tracked List に追加 / 削除する。     |
| `t`                 | All processes / Tracked only を切り替える。   |
| `Ctrl+T`            | 名前付き Tracked Lists を開く。                |
| `Ctrl+F`            | プロセス一覧を絞り込む。                     |
| `Ctrl+R`            | 記録を開始 / 停止する。                     |
| `Ctrl+L`            | 保存済みログを開く。                       |
| `?`                 | 全キー操作を表示する。                      |
| `q` / `Esc`         | 画面を戻る、または終了確認を開く。                   |

## 主な機能

- **モニタ**: RAM / VRAM、ネットワークとディスクの状態、平均 CPU 使用率と論理 CPU 別負荷を示すコンパクトな CPU パネル、プロセスごとの主要メトリクスをテーブル表示。ソート、列選択、フィルター、ジャンプ検索で対象を絞り込めます。
- **グラフ表示**: 選択したメトリクスを最大 4 つの Graph / Samples スロットに並べ、時系列の推移とサンプル値を確認できます。通常プロセスは約 120 秒、追跡中プロセスとシステム指標（RAM / VRAM、System Activity、平均 CPU 使用率）は約 7,200 秒の履歴を保持します。
- **追跡 (Tracked List)**: 関心のあるプロセス名を登録し、追跡中のものだけを表示できます。用途ごとのリストに名前を付けて保存・切り替えでき、起動時に前回の作業中リスト、保存済みリスト、空のリストから開始方法を選べます。プロセスが終了したあとも最後に取得した値が画面に残ります。RAM / VRAM、平均 CPU 使用率、System Activity は登録不要で常に履歴を保持します。
- **ログ記録と Log view**: 追跡中のプロセス、RAM / VRAM、平均 CPU 使用率、システム状態を JSON Lines ログとして保存し、あとから同じ Processes / Graph / Samples / A/B の画面構成で再調査できます。
- **A/B 比較**: 任意の 2 時点を A 点・B 点としてマークし、値の差分と経過時間を表示します。
- **プロセス調査**: 選択中プロセスのメトリクス、実行ファイル情報、現在開いているファイルを、レスポンシブなタブ式 Process Info ダイアログで確認できます。
- **操作支援**: `Ctrl+C` で選択行をクリップボードへコピー、`F2` でテーマ切替、マウスでの行選択やスクロールバー操作にも対応しています。

## こんなときに役立ちます

- アプリのメモリ使用量が継続的に増えていないか調べたい。
- 特定処理の前後でメモリやハンドル数がどれだけ変化したか確認したい。
- 現在開かれているファイルを確認し、クローズ漏れ調査の手がかりにしたい。
- バックグラウンドサービスを **長時間記録** し、現象が起きた付近を Log view で見直したい。
- リファクタの前後でリソース使用量を比較したい。

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
現行のパッケージ作成手順では、zip に `winproc-tui.exe` と `LICENSE` のみを含めます。README などのドキュメントは GitHub で公開し、配布zipには含めません。v0.4.0 のzipはこの方針より前に公開されたため、README、`assets/`、`docs/`も含まれます。

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

`f` のような 1 文字キーは、フォーカス中のパネルによって動作が変わります。Footer は現在のパネル名を重ねて表示せず、`? Help` を左端に置いて画面幅が狭い場合も残します。Live / Recording ではその次に `Ctrl+P Pause` を表示し、表示停止を利用できない Log view では省略します。推定しやすい Tab のフォーカス移動は Footer から省略します。下の表にはパネルごとの主要操作を掲載しています。

### 基本


| キー                  | 動作                             |
| ------------------- | ------------------------------ |
| `?`                 | ヘルプの表示 / 非表示。                  |
| `q` / `Esc`         | 終了確認を開く（Log view 中は live 表示へ戻る）。 |
| `Tab` / `Shift+Tab` | フォーカス移動。                       |
| `Ctrl+C`            | フォーカス中パネルの選択行テキストをコピー。         |
| `Ctrl+L`            | ログ一覧を開く。                       |
| `Ctrl+T`            | Tracked Lists を開き、組み込みの空リストのロード、名前付きリストの管理、起動時の扱いを設定する。 |
| `Ctrl+R`            | 記録の開始 / 停止。                    |
| `Ctrl+P`            | 表示更新の一時停止 / 再開。サンプリングと記録は継続する（Log view 中は使用不可）。 |
| `Ctrl+Wheel`        | Windows Terminal のズーム倍率を変更。     |
| `F2`                | テーマ切替。                         |


### プロセス操作


| キー                  | 動作                                      |
| ------------------- | --------------------------------------- |
| `Ctrl+F`            | プロセス名でフィルタリングする。`Full Path` 列を選択しているときは実行ファイルパスも対象にする。 |
| `Ctrl+I` / `Ctrl+J` | プロセス名のインクリメンタル検索。                       |
| `1` 〜 `4`           | 選択中のプロセス、RAM / VRAM、NW/DISK Activity、または CPU Usage メトリクスを Graph#1〜#4 に表示する（同じ番号を再押下で解除）。 |
| `0`                 | 全 Graph を解除して Graph パネルを閉じる。            |
| `s`                 | 選択カラムでソート（再押下で昇順 / 降順切替）。               |
| `c`                 | カラムピッカーを開く。                             |
| `Shift+Up/Down`     | 稼働中プロセス行を連続範囲で選択する。                     |
| `Ctrl+Up/Down`      | 複数選択を変えずにカーソルだけ移動する。                    |
| `Ctrl+Space`        | 現在の稼働中プロセス行を複数選択に追加 / 削除する。             |
| `Shift+Left/Right`  | 選択中のメトリクスカラムを左 / 右へ移動する。                |
| `w` / `Shift+W`     | 選択中カラムを 1 セル広げる / 狭める。                    |
| `Space`             | 選択プロセス名を Tracked List に追加 / 削除。         |
| `d` / `Delete`      | 確認後、選択した稼働中プロセス行を `taskkill /f /im` で終了する。 |
| `t`                 | 追跡中のみ表示するかを切り替える。                       |
| `Enter`             | 選択中プロセスの Process Info を開く。              |
| `i`                 | System Info ダイアログを開く。 |
| `f`                 | 選択中の稼働プロセスの Process Info を Files タブで開く。 |
| `g`                 | 設定済みの全 Graph を一括で開く / 閉じる。              |

Process Info は、大きいターミナルではコンパクトなまま、小さいターミナルでは利用可能領域に収まるレスポンシブなタブ式ダイアログです。`Metrics` は通常収集する 14 個の数値プロセスメトリクス、`Image` は実行ファイル、実行ユーザー、アーキテクチャ、完全なコマンドライン、バージョン情報、`Files` は従来の Open files 一覧、`DLLs` はロード済み DLL のフルパス一覧、`Environment` は対象プロセスの環境変数を表示します。`Ctrl+Right` / `Ctrl+Left` で次／前のタブへ切り替え、`Tab` / `Shift+Tab` で表示中タブの本文と Close ボタンの間を移動します。表示中のタブは Process Info を閉じても記憶され、同じ実行中に次回開いたとき復元されます。`f` で開いた場合は従来どおり `Files` を直接表示します。Graph または Samples で A 点と必要に応じて B 点を設定してから `Metrics` を開くと、A 点だけの場合は Current − A、両方ある場合は B − A を表示します。時刻が完全一致するサンプルがなければ `--` のままです。

通常の内容は `Up` / `Down`、`PageUp` / `PageDown`、`Home` / `End`、マウスホイールでスクロールします。`DLLs` と `Environment` では、これらのキーで行を選択し、`Enter` で選択中 DLL のメタデータ、または選択中環境変数の完全な値を開きます。長い詳細は同じキーでスクロールでき、`Esc` または `Enter` で一覧へ戻ります。`Files` と `DLLs` のフィルターはフルパスを対象にします。3 つの一覧フィルターは新しい Process Info を開くたびに消去しますが、同じダイアログ内でのタブ切り替えや更新では維持します。絞り込み中の件数表示では表示件数と総件数の両方を確認できます。`Image`、`Files`、`DLLs`、`Environment` では `Ctrl+U` で表示中タブを更新します。`Ctrl+C` は `Files` ではフィルター後のパス、`DLLs` では選択中 DLL のパス、`Environment` では選択中の `NAME=value` をコピーします。動的情報は操作時点の情報であり、保護されたプロセス、非対応のプロセス、終了済みプロセスでは取得できない場合があります。

環境変数の値には、パスワードやトークンなどの秘密情報が含まれる場合があります。値はタブを開いたとき、または明示的に更新したときだけ読み取り、ダイアログを閉じると Process Info の状態から消去します。Recording や Log view には保存しません。


### Graph と A/B 比較


| キー                         | 動作                           |
| -------------------------- | ---------------------------- |
| `Enter`                    | 操作中のプロセス Graph の Process Info を開く。 |
| `Left` / `Right`           | 選択サンプルを移動。                   |
| `Ctrl+Left` / `Ctrl+Right` | 表示範囲を左右に移動。                  |
| 右ドラッグ / `Ctrl`+左ドラッグ      | マウスで表示範囲を左右に移動。              |
| `PageUp` / `PageDown`      | 表示する時間幅を変更。                  |
| `f`                        | 全サンプルが収まる時間幅へ切り替え。           |
| `z`                        | Y 軸下限を 0 固定 / 表示範囲の最小値に切り替え。 |
| `v`                        | Samples テーブルの表示 / 非表示を切り替え。    |
| `d`                        | Samples の Delta 列の表示 / 非表示を切り替え。 |
| `l`                        | Graph 配置を Auto / 1 列 / 2 列の順に切り替え。 |
| `a` / `b`                  | 選択サンプルを A 点 / B 点としてマーク。     |
| `Shift+A` / `Shift+B`      | A 点 / B 点へジャンプ。              |
| `x`                        | A/B 比較をクリア。                  |


Graph 領域の上には、表示時間幅、カーソルと A/B の時刻に加え、`v: Samples`、`d: Delta`、現在の `l` 配置モード、`f: Fit all`、`z: Min 0` を 1 回だけ表示します。各操作はクリックでも切り替えられます。各スロットは `GRAPH#n · 対象 · メトリクス [単位] · B-A: 値` という 1 つの枠に Graph と同期した Samples テーブルをまとめ、操作中のスロットだけタイトルを強調し、ほかのスロットは控えめに表示します。`CPU%` のようにメトリクス名に単位が含まれる場合は重ねて表示しません。タイトルの `B-A` はそのスロット自身のメトリクス値で計算し、A / B のどちらかが未指定、または同時刻の値がない場合は `--` と表示します。
共通操作の `v`、`d`、`l`、`f`、`z` は、スロット内の Graph と Samples のどちらにフォーカスがあるときも使用できます。
プロセス Graph の Graph 部分にフォーカスがあるとき、`Enter` でそのスロットのプロセスに対する Process Info を開きます。Processes の選択行は変更しません。システム Graph にはプロセス詳細がありません。

Auto 配置は通常 1 列を使い、現在のターミナル領域で 2 列のほうが多くの割り当て済みスロットを表示できる場合だけ自動で 2 列にします。2 列ではスロットを左上、右上、左下、右下の順に並べます。Graph が 1 つだけなら横幅全体を使い、3 つなら右下は空きます。2 列表示では Samples を非表示にし、実効配置が 1 列へ戻ると直前の Samples 表示状態を復元します。`v` で Samples を有効にすると、固定 1 列モードへ切り替わります。

複数 Graph を表示するときは、表示時間幅、カーソル位置、A/B 点がスロット間で共有され、Y 軸スケール、サンプルの有無、値ラベルは Graph ごとに独立します。バイト値の Y 軸目盛りは `5.9 MB` のような可変単位で短く表示し、個数の目盛りは整数のまま表示します。短くなった共通の目盛り幅は各 Graph のプロット領域として使います。Samples、カーソルラベル、A/B の値と差分、クリップボード出力、記録ログは正確な値を維持します。Samples で選択した値が現在の Graph 表示範囲外にある場合は、その値が見える位置まで時間範囲を最小限移動します。Graph の追加やターミナル縮小で割り当ては削除しません。収まらないスロットは番号の大きいものから一時的に非表示になり、領域が広がると自動で再表示されます。

## 記録と Log view

`Ctrl+R` で記録の開始と停止を切り替えます。記録を開始するには Tracked List に 1 件以上の名前が必要です。ログは JSON Lines として保存されます（拡張子 `.log`）。各フレームには RAM / VRAM、平均 CPU 使用率、System Activity などのシステム指標と、Tracked List に一致する実行中プロセスが記録されます。一致するプロセスがその時点で存在しない場合も、システム指標は記録され、プロセス一覧は一致するプロセスが現れるまで空になります。記録開始時に保存先パスの入力ダイアログが開きます。保存先にはログのファイル名まで指定する必要があり、ディレクトリだけでは記録を開始できません。存在しない親ディレクトリは自動作成します。`Tab` / `Shift+Tab` でパスとボタンの間を移動し、パスにフォーカスがあるときは `Ctrl+Space` でディレクトリ名を補完できます。Log view 中は記録を開始できず、記録中は Log view を開けません。

`Ctrl+L` でログ一覧を開きます。前回の記録ディレクトリがあればそこ、なければカレントディレクトリの `*.log` のファイル名をコンパクトな一覧に表示します。`Dir` 行で検索中のディレクトリを確認でき、`d` または `Directory` ボタンで別ディレクトリを指定できます。マウス操作向けに `Open`、`Refresh`、`Close` ボタンも使用できます。選択したログを `Enter` で開くと表示が `LOG` に切り替わり、Processes / Graph / Samples / A/B 比較で過去のセッションを調査できます。
Log view は再生機能ではありません。Processes は記録の最終値を表示し続け、Graph、Samples、Process Info で記録済みメトリクスの履歴を確認します。Process Info の静的情報には記録済み項目を使い、記録されていない項目は `--` と表示します。`Esc` で Live 表示へ戻ります。

記録ログのフォーマットと各フィールドの意味は [docs/metrics.md](docs/metrics.md) を参照してください。

## 設定の保存

テーマ、Graph の配置と Samples / Delta の表示状態、プロセス表のカラム・ソート・幅、Tracked Only、作業中の Tracked List、保存済みの名前付きリストは自動的に保存され、次回起動時に復元されます。Tracked Lists の起動設定と Save、Rename、Delete の変更は、その操作時に保存されます。フィルター入力は次回起動に引き継ぎません。

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
