import { ApiError } from "./client";

const FIELD_LABELS: Record<string, string> = {
  name: "名稱",
  note: "備註",
  phone: "電話",
  address: "地址",
  price: "價格",
  description: "說明",
  category: "分類",
  email: "Email",
  displayName: "顯示名稱",
  title: "標題",
  deadlineAt: "截止時間",
  quantity: "數量",
};

const MESSAGES: Record<string, string> = {
  unauthorized: "請先登入",
  forbidden: "沒有權限執行此操作",
  bad_origin: "請求來源不被允許，請重新整理頁面後再試",
  invalid_name: "名稱不可為空白，且長度不可過長",
  duplicate_name: "清單中有重複的名稱",
  name_taken: "已有同名項目",
  invalid_status: "目前的狀態無法執行此操作，請重新整理後再試",
  group_archived: "這個團已封存，無法修改",
  vendor_name_taken: "已有同名店家",
  limit_exceeded: "已超過數量上限",
  not_found: "找不到資料，可能已被刪除",
  conflict: "資料剛被其他操作修改，請重新整理後再試",
  invalid_json: "送出的資料格式不正確，請重新整理頁面後再試",
  validation_failed: "輸入內容有誤，請檢查後再試",
  internal_error: "伺服器發生錯誤，請稍後再試",
  rate_limited: "操作太頻繁，請稍候再試",
  group_closed: "這個團已經關閉，無法再送出",
  deadline_passed: "已超過截止時間，無法再送出",
  sold_out: "有品項已售完，請重新選擇",
  invalid_credentials: "Email 或密碼不正確",
  oidc_not_configured: "尚未設定 OIDC 登入",
  provider_error: "無法連線到身分服務，請稍後再試",
  ai_not_configured: "尚未設定 AI 匯入功能",
  ai_quota_exceeded: "今日 AI 辨識次數已用完，請明天再試或改用手動輸入",
  image_too_large: "圖片太大（上限 5 MB），請換一張較小的照片",
  invalid_image: "無法讀取這張圖片，請改用 JPEG、PNG 或 WebP 照片",
  invalid_instruction: "指示文字過長（上限 200 字）",
  ai_refused: "這張照片無法辨識，請改用手動輸入",
  ai_unavailable: "AI 服務暫時無法使用，請稍後再試",
  invalid_changes: "有些變更內容不正確，請檢查標示的項目",
  invalid_state: "這份匯入已處理過，請重新整理頁面",
};

export const NETWORK_ERROR_MESSAGE = "網路連線失敗，請檢查網路後再試";
const FALLBACK_MESSAGE = "發生未預期的錯誤，請稍後再試";

/** zh-TW message for an error thrown by an API call. Never exposes raw English server detail. */
export function errorMessage(err: unknown): string {
  if (!(err instanceof ApiError)) {
    return err instanceof TypeError ? NETWORK_ERROR_MESSAGE : FALLBACK_MESSAGE;
  }
  const code = err.code;
  if (code) {
    if (MESSAGES[code]) return MESSAGES[code];
    if (code.startsWith("invalid_")) {
      const field = err.field ?? code.slice("invalid_".length);
      return `${FIELD_LABELS[field] ?? "欄位"}格式不正確`;
    }
    if (code.startsWith("duplicate_")) return MESSAGES.duplicate_name;
  }
  if (err.status === 429) return MESSAGES.rate_limited;
  if (err.status === 401) return MESSAGES.unauthorized;
  if (err.status === 404) return MESSAGES.not_found;
  if (err.status >= 500) return MESSAGES.internal_error;
  return FALLBACK_MESSAGE;
}

/**
 * If the error belongs to a form field, returns it so the form can show it inline.
 * `fields` lists the field names the form actually has.
 */
export function fieldError(
  err: unknown,
  fields: readonly string[],
): { field: string; message: string } | null {
  if (!(err instanceof ApiError) || !err.field || !fields.includes(err.field)) return null;
  if (err.status !== 409 && err.status !== 422) return null;
  return { field: err.field, message: errorMessage(err) };
}

const LOGIN_ERRORS: Record<string, string> = {
  invalid_state: "登入已逾時或重複使用，請重新登入",
  invalid_request: "登入請求不完整，請重新登入",
  email_not_verified: "您的身分服務帳號 Email 尚未驗證，無法登入",
  access_denied: "您取消了授權，未完成登入",
  provider_error: "無法連線到身分服務，請稍後再試",
  invalid_credentials: "Email 或密碼不正確",
  oidc_not_configured: "伺服器尚未設定 OIDC 登入",
  login_failed: "登入失敗，請再試一次",
};

/** Message for the `?error=` code the OAuth callback redirects with. */
export function loginErrorMessage(code: string): string {
  return LOGIN_ERRORS[code] ?? "登入時發生錯誤，請再試一次";
}
