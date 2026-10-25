import { useEffect, useState } from 'react'

import {
  Button,
  Spinner,
  TextField,
  useToast,
} from '../ui'
import { AdminApiError, adminApi, type AdminSettingsResponse } from './api'
import { AdminErrorBanner, AdminReauthNotice } from './AdminLayout'
import { bytesToGb, gbToBytes } from './format'

const MODES = [
  {
    value: 'approval',
    label: '需要审批',
    description: '新账号注册并验证邮箱后进入待审核状态，管理员批准后才能使用。',
  },
  {
    value: 'open',
    label: '开放注册',
    description: '新账号验证邮箱后立即可用，并使用默认额度。',
  },
  {
    value: 'closed',
    label: '关闭注册',
    description: '注册接口直接返回暂不开放注册，不创建账号。',
  },
] as const

export function AdminSettingsPage() {
  const [settings, setSettings] = useState<AdminSettingsResponse | null>(null)
  const [mode, setMode] = useState('approval')
  const [quotaGb, setQuotaGb] = useState('10')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const { showToast } = useToast()

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const response = await adminApi.getSettings()
        if (!cancelled) {
          setSettings(response)
          setMode(response.registration_mode)
          setQuotaGb(String(bytesToGb(response.default_quota_bytes)))
        }
      } catch (caught) {
        if (!cancelled) {
          setError(caught)
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  if (error instanceof AdminApiError && error.code === 'reauth_required') {
    return <AdminReauthNotice />
  }

  const save = async () => {
    const value = Number(quotaGb)
    if (!Number.isFinite(value) || value < 0) {
      showToast('请输入有效的默认额度', 'error')
      return
    }
    setBusy(true)
    try {
      const response = await adminApi.updateSettings(mode, gbToBytes(value))
      setSettings(response)
      showToast('设置已保存，立即生效', 'success')
      setError(null)
    } catch (caught) {
      setError(caught)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div data-testid="admin-settings" className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold text-ink-900">设置</h1>
        <p className="mt-1 text-sm text-ink-600">
          修改注册模式与新用户默认额度，保存后立即生效。
        </p>
      </header>

      <AdminErrorBanner error={error} />

      {loading ? (
        <div className="flex justify-center py-16">
          <Spinner size={26} label="加载设置" />
        </div>
      ) : (
        <section className="max-w-2xl space-y-6 rounded-card border border-line bg-white p-6">
          <fieldset>
            <legend className="text-base font-semibold text-ink-900">
              注册模式
            </legend>
            <div className="mt-3 space-y-2">
              {MODES.map((item) => (
                <label
                  key={item.value}
                  className="flex cursor-pointer items-start gap-3 rounded-control border border-line px-3 py-2.5 hover:bg-canvas"
                >
                  <input
                    type="radio"
                    name="registration-mode"
                    data-testid={`admin-registration-mode-${item.value}`}
                    className="mt-1"
                    value={item.value}
                    checked={mode === item.value}
                    onChange={() => setMode(item.value)}
                  />
                  <span>
                    <span className="block text-sm font-medium text-ink-900">
                      {item.label}
                    </span>
                    <span className="mt-0.5 block text-xs leading-5 text-ink-600">
                      {item.description}
                    </span>
                  </span>
                </label>
              ))}
            </div>
          </fieldset>

          <TextField
            label="新用户默认额度（GB）"
            data-testid="admin-default-quota"
            type="number"
            min={0}
            step="0.5"
            value={quotaGb}
            onChange={(event) => setQuotaGb(event.target.value)}
            hint="仅在批准或开放注册时使用；已批准用户的额度不会被批量修改。"
            containerClassName="max-w-xs"
          />

          <div className="flex items-center gap-3">
            <Button
              variant="primary"
              data-testid="admin-settings-save"
              disabled={busy}
              onClick={() => void save()}
            >
              {busy ? '保存中…' : '保存设置'}
            </Button>
            <p className="text-xs text-ink-500">
              当前：{settings?.registration_mode ?? mode} ·{' '}
              {settings
                ? `${bytesToGb(settings.default_quota_bytes)} GB`
                : '—'}
            </p>
          </div>
        </section>
      )}
    </div>
  )
}
