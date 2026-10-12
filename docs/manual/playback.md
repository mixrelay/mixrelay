# MixRelay — 일시정지와 재개 / Pause and resume

## 한국어

앱 하단 또는 MixRelay Driver 상태 메뉴에서 **일시정지**를 선택하면 모든 그래프의 라우팅과 오디오 처리가 멈춥니다. 앱과 Driver는 계속 실행되고 그래프 설정은 유지됩니다. **재개**를 선택하면 현재 소리부터 다시 처리합니다. 일시정지 중의 소리는 저장하거나 나중에 재생하지 않습니다.

진행 중인 녹음도 함께 일시정지하며 재개하면 **같은 파일에 이어서 기록**합니다. 일시정지 구간은 녹음 파일에서 빠지며 무음으로 채우지 않습니다. 예를 들어 10초 녹음 후 5초 일시정지하고 다시 10초 녹음하면 파일의 길이는 20초입니다. 녹음 시간 제한도 파일에 기록된 시간을 기준으로 합니다. 녹음기에서 직접 일시정지한 녹음, 중지한 녹음, 삭제한 녹음기는 전체 재개로 자동 시작하지 않습니다. 종료하면 열려 있는 녹음 파일을 마무리합니다.

MixRelay 앱에 연결된 실행부에서는 그래프 실행만 일시정지하며 가상 장치는 유지됩니다. 그래프 실행부를 사용하지 않는 독립 Driver 관리 모드에서는 가상 장치를 잠시 비활성화합니다. 이때 OS의 장치 목록에서 가상 장치가 사라졌다가 재개하면 기존 ID·이름·형식 설정으로 복원됩니다.

## English

Choose **Pause** in the app footer or MixRelay Driver status menu to pause routing and audio processing in all graphs. The app, Driver and graph settings remain available. **Resume** processes current audio; audio from the paused interval is discarded and is never replayed.

Active recordings pause and resume in **the same file**. The paused interval is omitted, without inserting silence: 10 seconds recorded, a 5-second pause, then 10 seconds recorded produces a 20-second file. Recording limits count recorded time. Recordings paused manually, stopped recordings and removed recorders do not restart automatically. Quitting finalizes open recordings.

The runtime connected to MixRelay pauses graphs while retaining virtual devices. In standalone Driver management mode, without the graph runtime, Pause temporarily disables virtual devices. They disappear from the OS device list and return on Resume with their existing IDs, names and format settings.

## 日本語

アプリ下部または MixRelay Driver のステータスメニューで **一時停止**を選ぶと、すべてのグラフのルーティングと音声処理が止まります。アプリ、Driver、グラフ設定は維持されます。**再開**すると現在の音声から処理します。一時停止中の音声は保存も再生もされません。

録音中のファイルは保持され、再開すると**同じファイルに続けて録音**します。一時停止区間は無音を挿入せず除外します。10秒録音、5秒一時停止、10秒録音ならファイルは20秒です。録音時間制限も記録された時間を数えます。手動で一時停止した録音、停止した録音、削除したレコーダーは自動再開しません。終了すると録音ファイルを確定します。

MixRelay に接続した実行部は仮想デバイスを維持します。グラフ実行部を使わない独立 Driver 管理モードでは仮想デバイスを一時的に無効にします。OSの一覧から消え、再開時に同じID、名前、形式設定で復元します。

## 简体中文

在应用底部或 MixRelay Driver 状态菜单选择**暂停**，可暂停所有图的路由和音频处理。应用、Driver 和图设置保持不变。选择**继续**后从当前音频开始处理；暂停期间的音频不会保存或回放。

正在进行的录音会暂停，继续时写入**同一个文件**。暂停区间会被省略，不会插入静音。例如录音10秒、暂停5秒、再录音10秒，文件长度为20秒。录音时长限制按实际记录时间计算。手动暂停、已停止的录音和已删除的录音节点不会自动继续。退出时会完成录音文件。

连接 MixRelay 的运行模块会保留虚拟设备。不使用图运行模块的独立 Driver 管理模式会暂时禁用虚拟设备。设备会从系统列表消失，继续时按原有ID、名称和格式设置恢复。

## 繁體中文

在應用程式底部或 MixRelay Driver 狀態選單選擇**暫停**，即可暫停所有圖的路由和音訊處理。應用程式、Driver 與圖設定仍會保留。選擇**繼續**後從目前音訊開始處理；暫停期間的音訊不會儲存或回放。

正在進行的錄音會暫停，繼續時寫入**同一個檔案**。暫停區間會被省略，不會插入靜音。例如錄音10秒、暫停5秒、再錄音10秒，檔案長度為20秒。錄音時長限制按實際記錄時間計算。手動暫停、已停止的錄音和已刪除的錄音節點不會自動繼續。結束時會完成錄音檔案。

連接 MixRelay 的執行部會保留虛擬裝置。不使用圖執行部的獨立 Driver 管理模式會暫時停用虛擬裝置。裝置會從系統清單消失，繼續時按原有ID、名稱和格式設定恢復。

## Français

Choisissez **Mettre en pause** en bas de l’application ou dans le menu de MixRelay Driver pour suspendre le routage et le traitement audio de tous les graphes. L’application, le Driver et les réglages restent disponibles. **Reprendre** traite le son actuel ; le son de la pause est écarté et ne sera pas rejoué.

Les enregistrements en cours reprennent dans **le même fichier**. La pause est omise, sans silence ajouté : 10 secondes enregistrées, 5 secondes de pause, puis 10 secondes enregistrées donnent un fichier de 20 secondes. Les limites comptent le temps enregistré. Les enregistrements mis en pause manuellement, arrêtés ou dont l’enregistreur a été supprimé ne redémarrent pas automatiquement. Quitter finalise les fichiers ouverts.

Le moteur connecté à MixRelay conserve les périphériques virtuels. En gestion autonome du Driver, sans moteur de graphes, la pause les désactive temporairement. Ils disparaissent de la liste système, puis reviennent avec leurs identifiants, noms et formats existants.

## Deutsch

Wählen Sie **Pausieren** unten in der App oder im Statusmenü von MixRelay Driver. Das Routing und die Audioverarbeitung aller Graphen werden angehalten. App, Driver und Einstellungen bleiben erhalten. **Fortsetzen** verarbeitet das aktuelle Audio; während der Pause wird kein Audio gespeichert oder später wiedergegeben.

Laufende Aufnahmen werden in **derselben Datei** fortgesetzt. Die Pause wird ausgelassen, ohne Stille einzufügen: 10 Sekunden Aufnahme, 5 Sekunden Pause und weitere 10 Sekunden Aufnahme ergeben eine Datei von 20 Sekunden. Aufnahmelimits zählen die aufgezeichnete Zeit. Manuell pausierte, gestoppte oder entfernte Aufnahmen starten nicht automatisch. Beenden schließt offene Aufnahmedateien ab.

Die mit MixRelay verbundene Laufzeit behält virtuelle Geräte bei. Die eigenständige Driver-Verwaltung ohne Graph-Laufzeit deaktiviert sie während der Pause. Sie verschwinden aus der Systemliste und werden mit bisherigen IDs, Namen und Formateinstellungen wiederhergestellt.

## Español

Seleccione **Pausar** en la parte inferior de la aplicación o en el menú de MixRelay Driver para detener temporalmente el enrutamiento y el procesamiento de todos los gráficos. La aplicación, el Driver y los ajustes se mantienen. **Reanudar** procesa el audio actual; el audio durante la pausa se descarta y no se reproduce después.

Las grabaciones activas continúan en **el mismo archivo**. La pausa se omite sin añadir silencio: 10 segundos grabados, una pausa de 5 segundos y otros 10 segundos grabados producen un archivo de 20 segundos. Los límites cuentan el tiempo grabado. Las grabaciones pausadas manualmente, detenidas o cuyos grabadores se hayan eliminado no se reanudan automáticamente. Salir finaliza los archivos abiertos.

El motor conectado a MixRelay conserva los dispositivos virtuales. La gestión independiente del Driver, sin motor de gráficos, los desactiva temporalmente. Desaparecen de la lista del sistema y vuelven con sus identificadores, nombres y formatos originales.

## Português

Selecione **Pausar** na parte inferior do aplicativo ou no menu do MixRelay Driver para suspender o roteamento e o processamento de todos os grafos. O aplicativo, o Driver e as configurações permanecem disponíveis. **Retomar** processa o áudio atual; o áudio da pausa é descartado e não será reproduzido depois.

As gravações em andamento continuam no **mesmo arquivo**. O intervalo da pausa é omitido, sem inserir silêncio: 10 segundos gravados, uma pausa de 5 segundos e mais 10 segundos gravados produzem um arquivo de 20 segundos. Os limites contam o tempo gravado. Gravações pausadas manualmente, encerradas ou cujos gravadores foram removidos não reiniciam automaticamente. Sair finaliza os arquivos abertos.

O motor conectado ao MixRelay mantém os dispositivos virtuais. O gerenciamento independente do Driver, sem motor de grafos, os desativa temporariamente. Eles desaparecem da lista do sistema e retornam com seus IDs, nomes e formatos existentes.

## Italiano

Seleziona **Pausa** in fondo all’app o nel menu di MixRelay Driver per sospendere l’instradamento e l’elaborazione di tutti i grafi. App, Driver e impostazioni restano disponibili. **Riprendi** elabora l’audio corrente; quello della pausa viene scartato e non sarà riprodotto in seguito.

Le registrazioni in corso proseguono nello **stesso file**. La pausa viene omessa senza inserire silenzio: 10 secondi registrati, 5 secondi di pausa e altri 10 secondi registrati producono un file di 20 secondi. I limiti contano il tempo registrato. Le registrazioni messe in pausa manualmente, terminate o i cui registratori sono stati rimossi non ripartono automaticamente. Uscire finalizza i file aperti.

Il motore collegato a MixRelay mantiene i dispositivi virtuali. La gestione indipendente del Driver, senza motore dei grafi, li disattiva temporaneamente. Scompaiono dall’elenco di sistema e tornano con gli stessi ID, nomi e formati.

## Русский

Выберите **Приостановить** внизу приложения или в меню MixRelay Driver, чтобы приостановить маршрутизацию и обработку всех графов. Приложение, Driver и настройки остаются доступными. **Возобновить** запускает обработку текущего звука; звук во время паузы отбрасывается и не воспроизводится позже.

Текущие записи продолжаются **в том же файле**. Интервал паузы исключается без добавления тишины: 10 секунд записи, 5 секунд паузы и ещё 10 секунд записи дают файл длиной 20 секунд. Ограничения учитывают записанное время. Записи, приостановленные вручную, остановленные записи и удалённые рекордеры автоматически не запускаются. Выход завершает открытые файлы.

Среда выполнения, подключённая к MixRelay, сохраняет виртуальные устройства. Автономное управление Driver без среды графов временно отключает их. Они исчезают из системного списка и возвращаются с прежними идентификаторами, именами и форматами.
