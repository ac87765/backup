/**
 * Crypto Worker 入口。
 *
 * 这是**唯一**加载 WASM 的地方：主线程通过 `client.ts` 的 Comlink 代理调用，
 * 永远拿不到密钥字节（见 `types.ts` 的契约）。
 *
 * 注意：`expose()` 必须**同步**执行——如果先 `await` 初始化再 expose，
 * Worker 启动期间主线程发来的第一条 RPC 会因为没有消息监听而丢失。因此这里把
 * “初始化 + 创建密钥库”的 Promise 交给 API，由每个方法自行等待就绪。
 */
import { expose } from 'comlink'

import { initCryptoWasm, WasmCryptoBackend } from './crypto-backend'
import { createCryptoApi } from './crypto-api'

const backend = initCryptoWasm().then(() => new WasmCryptoBackend())
expose(createCryptoApi(backend))


