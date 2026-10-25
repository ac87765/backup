import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { AuthLayout } from '../components/AuthLayout'
import { AppShell } from '../shell/AppShell'
import { PageHeader } from '../shell/PageHeader'

/**
 * “安全与隐私”页面（task 9.4 / client-encryption spec）。
 *
 * 这里必须如实说明服务端可见信息、Web E2EE 的固有限制，以及密码与恢复密钥
 * 同时丢失的后果。页面公开可读；登录后从侧边栏进入时仍在外壳内展示。
 */
function SecurityContent({ loggedIn }: { loggedIn: boolean }) {
  return (
    <article
      data-testid="security-page"
      className="space-y-8 text-sm leading-6 text-ink-600"
    >
      <section aria-labelledby="server-visible-heading" className="space-y-3">
        <h2
          id="server-visible-heading"
          className="text-base font-semibold text-ink-900"
        >
          服务端能看到的信息
        </h2>
        <p>
          Web 版是端对端加密：文件内容、文件名、文件元数据和主密钥都在你的浏览器里
          加解密。服务端（以及数据库、OSS 的运维者）仍然能看到下面这些
          <strong className="text-ink-900">元数据</strong>
          ，它们足以推断部分使用情况：
        </p>
        <ul
          data-testid="server-visible-list"
          className="list-disc space-y-1 pl-5"
        >
          <li>账号邮箱，以及邮箱是否已验证、注册与登录时间。</li>
          <li>
            文件夹树结构：节点数量、父子关系、回收站状态与版本数量；但看不到名称。
          </li>
          <li>
            每个版本的密文大小与块数、内容对象引用关系，以及配额用量。
          </li>
          <li>
            创建、修改、删除、恢复版本等操作的时间，以及文件版本的历史顺序。
          </li>
          <li>访问时间、IP 地址、User-Agent，以及会话的创建/最近使用/撤销时间。</li>
          <li>
            同名检测只比较由你的主密钥派生的 HMAC 令牌；服务端无法由令牌反推名称。
          </li>
        </ul>
        <p>
          除上述信息外，服务端不会收到明文文件内容、明文文件名、MIME 类型、原始修改时间、
          主密钥、节点密钥、恢复密钥或密码。服务端日志也不记录请求体、Cookie、会话令牌、
          密钥或预签名 URL 的签名部分。
        </p>
      </section>

      <section aria-labelledby="web-e2ee-heading" className="space-y-3">
        <h2
          id="web-e2ee-heading"
          className="text-base font-semibold text-ink-900"
        >
          Web 版 E2EE 的局限
        </h2>
        <div data-testid="web-e2ee-limits" className="space-y-2">
          <p>
            Web 应用每次打开都由服务端下发前端 JavaScript 与 WASM。端对端加密的前提是
            <strong className="text-ink-900">这些代码没有被篡改</strong>
            。如果服务端被攻破、或被恶意运营者控制，它可以下发窃取密码或主密钥的代码；
            浏览器端无法从数学上完全排除这种可能。
          </p>
          <p>
            我们通过严格 CSP（禁止内联脚本、只允许同源脚本与 WASM）、带内容哈希的资源名、
            发布构建可复现与公开构建哈希等措施降低风险，但这些措施只能降低、不能消除
            “信任服务端下发的代码”这一 Web 平台固有限制。
          </p>
          <p>
            此外，被攻破的浏览器、恶意浏览器扩展、键盘记录器或已被控制的设备同样可以
            读取你输入的密码。这类风险不在服务端控制范围内。
          </p>
          <p>
            长期方案是提供桌面客户端或浏览器扩展，让用户可校验/固定客户端代码；Web 版
            仍会保留，但会如实标注上述限制。
          </p>
        </div>
      </section>

      <section aria-labelledby="lost-keys-heading" className="space-y-3">
        <h2
          id="lost-keys-heading"
          className="text-base font-semibold text-ink-900"
        >
          密码与恢复密钥同时丢失会怎样
        </h2>
        <div
          data-testid="lost-keys"
          className="space-y-2 rounded-card border-l-4 border-warning-600 bg-warning-50 p-4 text-ink-900"
        >
          <p>
            主密钥（MK，用来解开你的文件内容）由你的密码或恢复密钥保护；服务端只保存
            加密后的密钥材料。服务端<strong>没有</strong>主密钥、也没有恢复密钥，因此
            <strong>无法</strong>帮你解密、重置或找回数据。
          </p>
          <p>
            如果忘记密码<strong>并且</strong>丢失恢复密钥，已有文件将永久无法解密；
            这属于密码学意义上的不可恢复，不是客服可以绕过的账号锁定。你只能重新注册
            一个新账号，旧数据仍然存在但任何人都无法读取。
          </p>
          <p>
            只忘记密码时，可用恢复密钥重置密码并重新获得访问；只丢失恢复密钥但记得密码时，
            仍可正常登录，并应尽快在设置中重新生成恢复密钥。
          </p>
        </div>
      </section>

      <section aria-labelledby="recovery-heading" className="space-y-3">
        <h2
          id="recovery-heading"
          className="text-base font-semibold text-ink-900"
        >
          恢复密钥保管建议
        </h2>
        <ul
          data-testid="recovery-advice"
          className="list-disc space-y-1 pl-5"
        >
          <li>
            注册时按分组抄写到纸张或金属助记板，并当面回填确认；不要只截图。
          </li>
          <li>
            至少准备两份，放在不同的物理位置（例如家中与保险箱/可信亲友处），避免火灾、
            水浸或单点丢失同时毁掉两份。
          </li>
          <li>
            可以存入经过审计的密码管理器，但不要与密码放在同一个条目、同一台未加密设备，
            也不要通过邮件、聊天工具或明文云笔记发送。
          </li>
          <li>
            恢复密钥等同于解密数据的凭据：不要给任何人，包括声称是客服或运维的人。
            服务端永远不会向你索要恢复密钥或密码。
          </li>
          <li>
            怀疑恢复密钥泄露时，立即用当前密码登录并轮换恢复密钥；轮换后旧密钥失效。
          </li>
          <li>
            定期（例如每半年）检查恢复密钥是否仍可读取，并在更换设备/密码管理器后确认备份。
          </li>
        </ul>
      </section>

      <nav className="flex flex-wrap gap-4 border-t border-line pt-4 text-sm">
        {loggedIn ? (
          <Link
            to="/files"
            className="font-medium text-brand-600 hover:underline"
          >
            返回文件
          </Link>
        ) : (
          <Link
            to="/login"
            className="font-medium text-brand-600 hover:underline"
          >
            返回登录
          </Link>
        )}
        <Link
          to="/register"
          className="font-medium text-brand-600 hover:underline"
        >
          注册新账号
        </Link>
      </nav>
    </article>
  )
}

export function SecurityPage() {
  const { user } = useAuth()

  if (user) {
    return (
      <AppShell title="安全与隐私">
        <div className="max-w-3xl space-y-6">
          <PageHeader
            title="安全与隐私"
            subtitle="端对端加密保护什么、不保护什么，以及你需要保管好什么。"
          />
          <SecurityContent loggedIn />
        </div>
      </AppShell>
    )
  }

  return (
    <AuthLayout
      title="安全与隐私"
      subtitle="端对端加密保护什么、不保护什么，以及你需要保管好什么。"
      size="lg"
    >
      <SecurityContent loggedIn={false} />
    </AuthLayout>
  )
}
