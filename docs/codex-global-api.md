# Codex 模型全域 API

RurixForge 的 Codex 模型透過獨立本機網關提供給同一台電腦、同一 Windows 使用者下的其他 agent。網關直接使用 RurixForge 後端既有的 `CODEX_HOME` 與登入狀態，不需要 Docker 或另開 OAuth 登入；呼叫會共用該 ChatGPT 方案的額度。

- Base URL：`http://127.0.0.1:8111/v1`
- 模型清單：`GET /v1/models`
- 推論：`POST /v1/responses` 或 `POST /v1/chat/completions`
- 驗證：`Authorization: Bearer <RURIXFORGE_API_KEY>`
- 預設模型：`codex/gpt-6.1-sol`

模型別名為 `codex/gpt-6.1-sol`、`codex/gpt-6-astra`、`codex/gpt-6-sol`、`codex/gpt-6-luna`、`codex/gpt-5.6-sol`、`codex/gpt-5.6-terra`、`codex/gpt-5.6-luna`。以實際 `/v1/models` 回傳及帳號可用額度為準。

網關只監聽 loopback；「全域」指跨本機專案可用。其他電腦無法使用此 `127.0.0.1` 位址，這項設定不會開放公網服務。

## 金鑰與環境

私人連線設定位於 `%USERPROFILE%\.config\rurixforge\codex-api.json`，包含 `apiKey`、`baseUrl`、`model`、`models`，以及後端的 `codexHome`、`codexBin` 路徑。API key 同時儲存在 Windows 使用者環境變數 `RURIXFORGE_API_KEY`；另有 `RURIXFORGE_BASE_URL`、`RURIXFORGE_MODEL`。上游 Codex 登入憑證保留在原本的 `auth.json`，呼叫端只需網關金鑰。

網關每次請求讀取同一份 `auth.json`。登入即將過期或上游回傳 401 時，會透過同一個官方 Codex 執行檔與同一個 `CODEX_HOME` 更新既有登入；不複製或另存 refresh token，也不啟動新的登入流程。

既有程式不會自動取得更新後的使用者環境變數。重新啟動 agent/終端，或在目前 PowerShell 執行：

```powershell
. D:\RurixForge\tools\codex-global-api\use-rurixforge.ps1
```

需要標準 OpenAI 環境變數的 agent 可加 `-OpenAI`。這只會設定目前程序及後續子程序的 `OPENAI_API_KEY`、`OPENAI_BASE_URL`，不會覆蓋 Windows 使用者層級的原有值：

```powershell
. D:\RurixForge\tools\codex-global-api\use-rurixforge.ps1 -OpenAI
```

## Codex CLI

`configure-client.ps1` 在 `%USERPROFILE%\.codex\config.toml` 新增 `rurixforge` provider，並在相鄰的 `%USERPROFILE%\.codex\rurixforge.config.toml` 建立目前 Codex 使用的獨立 profile，保留原本預設模型。它會移除舊版工具在管理區塊內建立的 `[profiles.rurixforge]` 表格；每次實際修改前保留獨立備份，重複執行不產生額外變更。若已有同名但非本工具管理的設定，會停止並保留原檔。

```powershell
powershell -NoProfile -File D:\RurixForge\tools\codex-global-api\configure-client.ps1
. D:\RurixForge\tools\codex-global-api\use-rurixforge.ps1
codex --profile rurixforge
```

Profile 使用 Responses API 與 `medium` 推理強度；可用 `codex --profile rurixforge --model codex/gpt-6-astra` 切換模型。

## 其他 agent / Python

在已載入環境變數的程序中，使用 OpenAI 相容客戶端：

```python
import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["RURIXFORGE_API_KEY"],
    base_url=os.environ["RURIXFORGE_BASE_URL"],
)
response = client.responses.create(
    model=os.environ["RURIXFORGE_MODEL"],
    input="請回覆 READY。",
)
print(response.output_text)
```

API key 不應放入原始碼、Git、命令列參數或交接記錄。輪替本機金鑰時，須同步更新私人設定、使用者環境變數與呼叫端，並重新啟動網關。

## 啟動與維護

安裝程式只編譯獨立 Go 網關，將執行檔與啟動腳本放在 `%LOCALAPPDATA%\RurixForge\CodexApi`，並建立目前 Windows 使用者的登入啟動捷徑。無須開啟 RurixForge 視窗；既有 Codex 登入檔與執行檔仍須存在。

```powershell
& D:\RurixForge\tools\codex-global-api\install.ps1
& D:\RurixForge\tools\codex-global-api\service.ps1 status
& D:\RurixForge\tools\codex-global-api\service.ps1 restart
py -3 D:\RurixForge\tools\codex-global-api\verify.py
```

重新掃描模型或後端路徑：執行 `setup.py`，再執行 `configure-client.ps1` 與 `service.ps1 restart`。輪替網關 key：`py -3 D:\RurixForge\tools\codex-global-api\setup.py --rotate-key`，接著重新啟動網關與呼叫端。取消自動啟動可移除使用者 Startup 資料夾中的 `RurixForge Codex API.lnk`；`service.ps1 stop` 會停止目前網關。

目前提供文字、圖片輸入與 function tools 的 Chat/Responses 轉接，最多 4 個並行請求，預設每次請求限時 600 秒。私人 JSON 可設 `requestTimeoutSeconds` 調整至最多 3600 秒，修改後重啟網關。`store=false`，多輪請求須攜帶歷史與工具結果；不提供伺服器端 `previous_response_id` 對話保存。Codex 訂閱上游不接受的 `temperature`、`top_p`、`max_output_tokens` 會移除，`max_output_tokens` 不會成為實際輸出上限。若原登入被撤銷，請在 RurixForge 重新登入後重跑 `setup.py`。
