<script setup lang="ts">
/**
 * 设置页 — 需求 3.3.4 / 3.14 / DESIGN 15.6 / 20
 * 上半：启动 / 停止热键捕获 + 保存（保存按钮属于热键卡片，仅持久化热键）。
 * 下半：提示音录制。初始每项提供「试听 / 录制」；点「录制」展开统一面板：
 *   顶部完整波形 + 下方五个按钮（开始录制 / 结束录制 / 试听 / 保存 / 取消）。
 *   录制中实时波形；停止（或 5s 自动）后波形静态，叠加可拖动的起始 / 结束标记，
 *   「试听」播放选区并显示移动进度线，「保存」裁剪写盘，「取消」丢弃并关闭面板。
 */
import { ref, onMounted, onBeforeUnmount, computed, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { appStore } from '../stores/appStore'
import { markCurrentConfigPersisted } from '../lib/configUtil'
import { commandError, commandErrorMessage } from '../lib/errorUtil'
import KeyCaptureInput from '../components/KeyCaptureInput.vue'
import type { CapturedKey, HotkeyConfig, HotkeyUpdateResult, LogLevel } from '../types/config'

/** 已持久化快照（用于保存时对比是否真的变更） */
const persistedSnapshot = ref<HotkeyConfig>(cloneHotkeys(appStore.hotkeys))

/** v-model 绑定到 reactive store；阶段 6 暂用本地副本，避免捕获后立即影响快照对比 */
const startKey = ref<CapturedKey | null>(cloneKey(appStore.hotkeys.start))
const stopKey = ref<CapturedKey | null>(cloneKey(appStore.hotkeys.stop))

/** 保存提示文本：null 表示不显示；保存后短暂显示，失败/无变化时不显示 */
const saveMessage = ref<string | null>(null)
/** 提示文本是否为警告（橙色） */
const isWarning = ref(false)
let messageTimer: number | null = null
const persistedLogLevel = ref<LogLevel>(appStore.logLevel)
const logLevelSaving = ref(false)
const logLevelMessage = ref<string | null>(null)

async function onLogLevelChange(): Promise<void> {
  const previous = persistedLogLevel.value
  const next = appStore.logLevel
  logLevelSaving.value = true
  logLevelMessage.value = null
  try {
    await invoke('update_log_level', { level: next })
    persistedLogLevel.value = next
    markCurrentConfigPersisted()
    logLevelMessage.value = '已生效'
  } catch (error) {
    appStore.logLevel = previous
    logLevelMessage.value = '修改失败'
    console.error('Failed to update log level:', error)
  } finally {
    logLevelSaving.value = false
  }
}

const isDirty = computed(() => {
  if (!startKey.value || !stopKey.value) return false
  return (
    startKey.value.keyLabel !== persistedSnapshot.value.start.keyLabel ||
    startKey.value.scanCode !== persistedSnapshot.value.start.scanCode ||
    stopKey.value.keyLabel !== persistedSnapshot.value.stop.keyLabel ||
    stopKey.value.scanCode !== persistedSnapshot.value.stop.scanCode
  )
})

function cloneKey(k: CapturedKey): CapturedKey {
  return { keyLabel: k.keyLabel, scanCode: k.scanCode }
}

function cloneHotkeys(h: HotkeyConfig): HotkeyConfig {
  return { start: cloneKey(h.start), stop: cloneKey(h.stop) }
}

function showMessage(text: string, warning: boolean = false): void {
  saveMessage.value = text
  isWarning.value = warning
  if (messageTimer !== null) {
    window.clearTimeout(messageTimer)
  }
  messageTimer = window.setTimeout(() => {
    saveMessage.value = null
    isWarning.value = false
    messageTimer = null
  }, 2000)
}

async function onSave(): Promise<void> {
  if (!startKey.value || !stopKey.value) return
  if (!isDirty.value) {
    // 无变化不提示（需求 3.3.4）
    return
  }

  try {
    // 阶段 12：调用 update_hotkeys 注册 + 持久化
    const result = await invoke<HotkeyUpdateResult>('update_hotkeys', {
      hotkeys: {
        start: cloneKey(startKey.value),
        stop: cloneKey(stopKey.value),
      },
    })

    // 无变化：不提示
    if (!result.changed) return

    // 注册失败
    if (!result.registered) {
      const msg = result.message || '热键注册失败'
      showMessage(msg, msg.includes('冲突'))
      return
    }

    // 持久化失败
    if (!result.persisted) {
      showMessage('持久化失败', false)
      return
    }

    // 注册 + 持久化都成功
    appStore.hotkeys = {
      start: cloneKey(startKey.value),
      stop: cloneKey(stopKey.value),
    }
    persistedSnapshot.value = cloneHotkeys(appStore.hotkeys)
    markCurrentConfigPersisted()
    showMessage('已保存', false)
  } catch (err) {
    const errorMsg = commandErrorMessage(err)
    showMessage(`保存失败: ${errorMsg}`, false)
    console.error('Failed to update hotkeys:', err)
  }
}

// ===== 提示音录制（阶段 18 / 18.1，统一面板） =====

type SoundTarget = 'start' | 'stop'

const MAX_REC_SECS = 5
const MIN_SELECTION_MS = 100 // 需求 3.14：最短 100ms

/** 文件是否已存在 */
const startExists = ref(false)
const stopExists = ref(false)
/** 麦克风是否可用（录制失败 no_input_device 时置 false 并禁用按钮） */
const micUnavailable = ref(false)
/** 录制区块提示文字 */
const recMessage = ref<string | null>(null)
const recIsWarning = ref(false)
let recMsgTimer: number | null = null

/** 统一面板：当前打开的目标；null 表示未展开（仅显示初始 试听/录制） */
const panelTarget = ref<SoundTarget | null>(null)
/** 是否正在采集 */
const isRecording = ref(false)
/** 录制已用时长（毫秒，前端计时仅作显示） */
const recElapsedMs = ref(0)

/** 录制完成后的缓冲（用于剪裁 / 试听 / 保存）；null 表示尚未录制 */
const recordedSamples = ref<Int16Array | null>(null)
const recSampleRate = ref(44100)
const recDurationMs = ref(0)
const trimStart = ref(0)
const trimEnd = ref(0)
const draggingMarker = ref<'start' | 'end' | null>(null)
/** 试听进度（毫秒）；null 表示未在播放 */
const playbackMs = ref<number | null>(null)

const hasRecording = computed(() => recordedSamples.value !== null)
/** 模拟运行 / 拾取期间禁用整个区块（Recording 不算忙） */
const simulationBusy = computed(() =>
  ['RunningKeyboard', 'RunningMouse', 'PickingMouse'].includes(appStore.runtimeStatus)
)

/** 波形 canvas（录制实时 + 静态共用；同一时刻仅一个面板渲染，故共用一个 ref） */
const waveCanvas = ref<HTMLCanvasElement | null>(null)
/** 录制 / 剪裁面板 DOM ref（用于面板出现时自动滚动到可视区） */
const recPanelEl = ref<HTMLElement | null>(null)
let rafId: number | null = null
let recTimer: number | null = null

/** 波形环形缓冲（最近 N 个幅度采样，0~1，仅录制中使用） */
const WAVE_LEN = 150
const waveBuf = new Array<number>(WAVE_LEN).fill(0)
let waveWritePos = 0

/** 试听播放上下文 */
let audioCtx: AudioContext | null = null
let playSrc: AudioBufferSourceNode | null = null
let playRaf: number | null = null

let unlistenAmp: UnlistenFn | null = null
let unlistenFinished: UnlistenFn | null = null
let unlistenError: UnlistenFn | null = null

const recElapsedLabel = computed(() => {
  const s = Math.min(Math.floor(recElapsedMs.value / 1000), MAX_REC_SECS)
  return `0:0${s} / 0:0${MAX_REC_SECS}`
})

const trimRangeLabel = computed(() => {
  const s = (trimStart.value / 1000).toFixed(1)
  const e = (trimEnd.value / 1000).toFixed(1)
  const dur = ((trimEnd.value - trimStart.value) / 1000).toFixed(1)
  return `已选 ${s}s ~ ${e}s（${dur}秒）`
})

/** 标记 / 进度线在波形上的百分比定位 */
function markerLeft(ms: number): string {
  if (recDurationMs.value <= 0) return '0%'
  return (ms / recDurationMs.value) * 100 + '%'
}

function showRecMessage(text: string, warning = false): void {
  recMessage.value = text
  recIsWarning.value = warning
  if (recMsgTimer !== null) window.clearTimeout(recMsgTimer)
  recMsgTimer = window.setTimeout(() => {
    recMessage.value = null
    recIsWarning.value = false
    recMsgTimer = null
  }, 2500)
}

async function refreshSoundStatus(): Promise<void> {
  try {
    const [s, t] = await invoke<[boolean, boolean]>('get_sound_status')
    startExists.value = s
    stopExists.value = t
  } catch (err) {
    console.error('Failed to get sound status:', err)
  }
}

/** 播放已存在的提示音文件（初始行「试听」） */
async function onPreviewFile(target: SoundTarget): Promise<void> {
  try {
    await invoke('preview_sound', { target })
  } catch (err) {
    console.error('preview_sound failed:', err)
  }
}

/** 点「录制」展开统一面板，但不立即开始采集（由用户点「开始录制」触发） */
function onOpenRecordPanel(target: SoundTarget): void {
  if (panelTarget.value !== null) return
  panelTarget.value = target
  isRecording.value = false
  recordedSamples.value = null
  recElapsedMs.value = 0
  playbackMs.value = null
  nextTick(() => {
    drawIdleWave()
    // 设置页高度受限，下方面板默认在视口外；展开后滚动到面板底部可见，
    // 让波形 + 按钮一并出现在用户视野内。
    recPanelEl.value?.scrollIntoView({ behavior: 'smooth', block: 'end' })
  })
}

/** 开始 / 重新开始采集 */
async function onStartRecord(): Promise<void> {
  if (panelTarget.value === null || isRecording.value) return
  const target = panelTarget.value
  try {
    await invoke('start_recording', { target })
    isRecording.value = true
    recordedSamples.value = null // 重录丢弃旧缓冲
    playbackMs.value = null
    recElapsedMs.value = 0
    waveBuf.fill(0)
    waveWritePos = 0
    appStore.runtimeStatus = 'Recording'
    const startTs = Date.now()
    recTimer = window.setInterval(() => {
      recElapsedMs.value = Date.now() - startTs
      if (recElapsedMs.value >= MAX_REC_SECS * 1000) {
        // 后端到上限也会自停；此处主动停一次保证 UI 一致。
        // 先清计时器，避免在 recording_finished 回来前重复 invoke('stop_recording')。
        if (recTimer !== null) {
          window.clearInterval(recTimer)
          recTimer = null
        }
        void onStopRecord()
      }
    }, 100)
    await nextTick()
    startWaveLoop()
  } catch (err) {
    isRecording.value = false
    const detail = commandError(err)
    if (detail.code === 'no_input_device') {
      micUnavailable.value = true
      showRecMessage('未检测到麦克风', true)
    } else {
      showRecMessage(`录制失败: ${detail.message}`, true)
    }
    console.error('start_recording failed:', err)
  }
}

/** 结束采集（保存缓冲，进入剪裁）；收尾在 recording_finished 事件 */
async function onStopRecord(): Promise<void> {
  if (!isRecording.value) return
  try {
    await invoke('stop_recording')
  } catch (err) {
    console.error('stop_recording failed:', err)
  }
}

/** 取消：录制中先取消采集（关闭在 finished 事件），否则直接关闭面板 */
async function onCancelPanel(): Promise<void> {
  if (isRecording.value) {
    try {
      await invoke('cancel_recording')
    } catch (err) {
      console.error('cancel_recording failed:', err)
    }
    return
  }
  closePanel()
}

/** 关闭面板并复位本地状态 */
function closePanel(): void {
  stopPlayback()
  stopRecLoop()
  panelTarget.value = null
  isRecording.value = false
  recordedSamples.value = null
  playbackMs.value = null
  if (appStore.runtimeStatus === 'Recording') appStore.runtimeStatus = 'Idle'
}

/** 停止录制计时器与实时重绘 */
function stopRecLoop(): void {
  if (recTimer !== null) {
    window.clearInterval(recTimer)
    recTimer = null
  }
  if (rafId !== null) {
    cancelAnimationFrame(rafId)
    rafId = null
  }
}

// ===== 波形绘制 =====
/** 待录制：仅画一根居中横线，避免初始空白；与录制中柱状波形在 0 幅度时视觉一致。 */
function drawIdleWave(): void {
  const canvas = waveCanvas.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  const cw = canvas.clientWidth
  if (canvas.width !== cw) canvas.width = cw
  const w = canvas.width
  const h = canvas.height
  ctx.clearRect(0, 0, w, h)
  const accent = getComputedStyle(canvas).getPropertyValue('--accent').trim()
  ctx.fillStyle = accent || '#FE7733'
  // 横线高度 1px，居中
  ctx.fillRect(0, h / 2, w, 1)
}

function startWaveLoop(): void {
  if (rafId !== null) return
  const draw = () => {
    drawRecordingWave()
    rafId = requestAnimationFrame(draw)
  }
  rafId = requestAnimationFrame(draw)
}

/** 录制中：环形缓冲实时柱状波形 */
function drawRecordingWave(): void {
  const canvas = waveCanvas.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  const cw = canvas.clientWidth
  if (canvas.width !== cw) canvas.width = cw
  const w = canvas.width
  const h = canvas.height
  ctx.clearRect(0, 0, w, h)
  const mid = h / 2
  const barW = w / WAVE_LEN
  const accent = getComputedStyle(canvas).getPropertyValue('--accent').trim()
  ctx.fillStyle = accent || '#FE7733'
  for (let i = 0; i < WAVE_LEN; i++) {
    const idx = (waveWritePos + i) % WAVE_LEN
    const barH = Math.max(1, waveBuf[idx] * (h - 2))
    ctx.fillRect(i * barW, mid - barH / 2, Math.max(1, barW - 1), barH)
  }
}

/** 录制完成：全长静态波形 + 选区遮罩（标记 / 进度线为 HTML 叠加层） */
function drawStaticWave(): void {
  const canvas = waveCanvas.value
  const samples = recordedSamples.value
  if (!canvas || !samples) return
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  const cw = canvas.clientWidth
  if (canvas.width !== cw) canvas.width = cw
  const w = canvas.width
  const h = canvas.height
  ctx.clearRect(0, 0, w, h)

  ctx.strokeStyle = getComputedStyle(canvas).getPropertyValue('--accent').trim() || '#FE7733'
  ctx.fillStyle = ctx.strokeStyle
  // 每像素列取该区间样本的绝对峰值，画成关于中线对称的细条（包络风格）
  const mid = h / 2
  const half = mid - 2
  const n = samples.length
  for (let x = 0; x < w; x++) {
    const i0 = Math.floor((x / w) * n)
    const i1 = Math.max(i0 + 1, Math.floor(((x + 1) / w) * n))
    let peak = 0
    for (let i = i0; i < i1; i++) {
      const v = samples[i]
      const a = v < 0 ? -v : v
      if (a > peak) peak = a
    }
    const barH = Math.max(1, (peak / 32768) * half)
    ctx.fillRect(x, mid - barH, 1, barH * 2)
  }

  // 选区外半透明遮罩
  ctx.fillStyle = 'rgba(0,0,0,0.5)'
  const startX = (trimStart.value / recDurationMs.value) * w
  const endX = (trimEnd.value / recDurationMs.value) * w
  ctx.fillRect(0, 0, startX, h)
  ctx.fillRect(endX, 0, w - endX, h)
}

// ===== 试听选区（Web Audio，带移动进度线） =====
async function onPreviewSelection(): Promise<void> {
  const samples = recordedSamples.value
  if (!samples) return
  stopPlayback()
  if (!audioCtx) audioCtx = new AudioContext()
  const ctx = audioCtx
  const buf = ctx.createBuffer(1, samples.length, recSampleRate.value)
  const ch = buf.getChannelData(0)
  for (let i = 0; i < samples.length; i++) ch[i] = samples[i] / 32768
  const src = ctx.createBufferSource()
  src.buffer = buf
  src.connect(ctx.destination)
  const startS = trimStart.value / 1000
  const durS = (trimEnd.value - trimStart.value) / 1000
  const baseTime = ctx.currentTime
  src.start(0, startS, durS)
  playSrc = src
  playbackMs.value = trimStart.value
  src.onended = () => stopPlayback()
  const tick = () => {
    const elapsedMs = (ctx.currentTime - baseTime) * 1000
    const cur = trimStart.value + elapsedMs
    if (cur >= trimEnd.value) {
      stopPlayback()
      return
    }
    playbackMs.value = cur
    playRaf = requestAnimationFrame(tick)
  }
  playRaf = requestAnimationFrame(tick)
}

function stopPlayback(): void {
  if (playSrc) {
    try {
      playSrc.onended = null
      playSrc.stop()
    } catch {
      // 已停止，忽略
    }
    playSrc = null
  }
  if (playRaf !== null) {
    cancelAnimationFrame(playRaf)
    playRaf = null
  }
  playbackMs.value = null
}

/** 保存：裁剪选区写盘覆盖文件 */
async function onSaveTrim(): Promise<void> {
  if (panelTarget.value === null || !hasRecording.value) return
  try {
    await invoke('save_trimmed_audio', {
      target: panelTarget.value,
      startMs: Math.round(trimStart.value),
      endMs: Math.round(trimEnd.value),
    })
    closePanel()
    await refreshSoundStatus()
    showRecMessage('已保存', false)
  } catch (err) {
    showRecMessage(`保存失败: ${err}`, true)
  }
}

function onMarkerMouseDown(marker: 'start' | 'end', e: MouseEvent): void {
  e.preventDefault()
  stopPlayback()
  draggingMarker.value = marker

  const onMove = (me: MouseEvent) => {
    if (!draggingMarker.value) return
    const canvas = waveCanvas.value
    if (!canvas) return
    const rect = canvas.getBoundingClientRect()
    const ratio = (me.clientX - rect.left) / rect.width
    const ms = Math.max(0, Math.min(recDurationMs.value, ratio * recDurationMs.value))
    if (draggingMarker.value === 'start') {
      // 夹到 [0, trimEnd - MIN]；录制时长 < MIN 时退化为 0，避免负值
      trimStart.value = Math.max(0, Math.min(ms, trimEnd.value - MIN_SELECTION_MS))
    } else {
      // 夹到 [trimStart + MIN, recDurationMs]；避免越过画布右界
      trimEnd.value = Math.min(recDurationMs.value, Math.max(ms, trimStart.value + MIN_SELECTION_MS))
    }
    drawStaticWave()
  }
  const onUp = () => {
    draggingMarker.value = null
    window.removeEventListener('mousemove', onMove)
    window.removeEventListener('mouseup', onUp)
  }
  window.addEventListener('mousemove', onMove)
  window.addEventListener('mouseup', onUp)
}

/** 解码 base64 PCM → Int16Array */
function decodeSamples(base64: string): Int16Array {
  const bin = atob(base64)
  const buf = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i)
  return new Int16Array(buf.buffer)
}

onMounted(async () => {
  await refreshSoundStatus()

  unlistenAmp = await listen<{ level: number }>('recording_amplitude', (e) => {
    waveBuf[waveWritePos] = Math.min(1, Math.max(0, e.payload.level))
    waveWritePos = (waveWritePos + 1) % WAVE_LEN
  })

  unlistenFinished = await listen<{
    cancelled: boolean
    samplesBase64?: string
    sampleRate?: number
    durationMs: number
  }>('recording_finished', (e) => {
    stopRecLoop()
    isRecording.value = false
    if (e.payload.cancelled) {
      closePanel()
      return
    }
    // 录制完成：缓冲就绪，进入同面板的剪裁子态
    if (e.payload.samplesBase64 && panelTarget.value) {
      recordedSamples.value = decodeSamples(e.payload.samplesBase64)
      recSampleRate.value = e.payload.sampleRate || 44100
      recDurationMs.value = e.payload.durationMs
      trimStart.value = 0
      trimEnd.value = e.payload.durationMs
      appStore.runtimeStatus = 'Idle'
      nextTick(() => drawStaticWave())
    } else {
      closePanel()
    }
  })

  unlistenError = await listen<{ error: string }>('recording_error', (e) => {
    stopRecLoop()
    isRecording.value = false
    appStore.runtimeStatus = 'Idle'
    const err = e.payload.error
    if (err.includes('no_input_device')) {
      micUnavailable.value = true
      showRecMessage('未检测到麦克风', true)
    } else {
      showRecMessage(`录制出错: ${err}`, true)
    }
  })
})

onBeforeUnmount(() => {
  // 离开设置页时若仍在录制，取消之
  if (isRecording.value) {
    void invoke('cancel_recording').catch(() => {})
  }
  stopPlayback()
  stopRecLoop()
  if (audioCtx) {
    void audioCtx.close().catch(() => {})
    audioCtx = null
  }
  if (recMsgTimer !== null) window.clearTimeout(recMsgTimer)
  if (messageTimer !== null) window.clearTimeout(messageTimer)
  unlistenAmp?.()
  unlistenFinished?.()
  unlistenError?.()
})

onMounted(() => {
  // 设置页不是可触发模拟页 → runtimeStatus 维持 Idle
  appStore.runtimeStatus = 'Idle'
})
</script>

<template>
  <section class="settings-page">
    <header class="page-header">
      <h2 class="page-title">全局热键</h2>
      <p class="page-desc">在按键模拟 / 鼠标模拟页按下热键启动或停止模拟。</p>
    </header>

    <div class="form">
      <div class="form-row">
        <label class="form-label">启动热键</label>
        <KeyCaptureInput v-model="startKey" placeholder="点击捕获按键" />
      </div>

      <div class="form-row">
        <label class="form-label">停止热键</label>
        <KeyCaptureInput v-model="stopKey" placeholder="点击捕获按键" />
      </div>

      <footer class="form-footer">
        <span v-if="saveMessage" class="save-msg" :class="{ 'msg-warning': isWarning }">{{ saveMessage }}</span>
        <button
          type="button"
          class="save-btn"
          :disabled="!isDirty || isRecording"
          @click="onSave"
        >
          保存
        </button>
      </footer>
    </div>

<header class="page-header section-gap">
      <h2 class="page-title">日志记录</h2>
      <p class="page-desc">日志写入 data/logs，修改后立即生效；生产环境默认仅记录错误。</p>
    </header>

    <div class="form">
      <div class="form-row">
        <label class="form-label" for="log-level">日志等级</label>
        <span class="log-control">
          <span v-if="logLevelMessage" class="save-msg">{{ logLevelMessage }}</span>
          <select id="log-level" v-model="appStore.logLevel" class="log-select" :disabled="logLevelSaving || isRecording" @change="onLogLevelChange">
            <option value="error">错误</option>
            <option value="warn">警告</option>
            <option value="info">信息</option>
            <option value="debug">调试</option>
          </select>
        </span>
      </div>
    </div>
    <header class="page-header section-gap">
      <h2 class="page-title">提示音</h2>
      <p class="page-desc">录制人声覆盖 data/audio 中的 按键开启 / 按键关闭 .wav（最长 5 秒）。</p>
    </header>

    <div class="form sound-form">
      <!-- 开启提示音 -->
      <div class="sound-item">
        <div class="sound-line">
          <span class="dot" :class="panelTarget === 'start' ? 'dot-rec' : startExists ? 'dot-on' : 'dot-off'"></span>
          <span class="sound-label">开启提示音</span>
          <span class="sound-state">
            <template v-if="panelTarget === 'start'">录制 / 剪裁中</template>
            <template v-else-if="startExists">已录制</template>
            <template v-else>未录制</template>
          </span>
          <span class="sound-actions" v-if="panelTarget !== 'start'">
            <button type="button" class="mini-btn" :disabled="!startExists || panelTarget !== null || simulationBusy" @click="onPreviewFile('start')">试听</button>
            <button type="button" class="mini-btn rec" :disabled="panelTarget !== null || micUnavailable || simulationBusy" @click="onOpenRecordPanel('start')">录制</button>
          </span>
        </div>
        <div v-if="panelTarget === 'start'" ref="recPanelEl" class="rec-panel">
          <span class="rec-status">
            <template v-if="isRecording">录制中 {{ recElapsedLabel }}</template>
            <template v-else-if="hasRecording">{{ trimRangeLabel }}</template>
            <template v-else>点击「开始录制」开始</template>
          </span>
          <div class="wave-wrap">
            <canvas ref="waveCanvas" class="wave" height="60"></canvas>
            <template v-if="!isRecording && hasRecording">
              <div class="trim-marker start" :style="{ left: markerLeft(trimStart) }" @mousedown="onMarkerMouseDown('start', $event)">
                <div class="marker-handle"></div>
              </div>
              <div class="trim-marker end" :style="{ left: markerLeft(trimEnd) }" @mousedown="onMarkerMouseDown('end', $event)">
                <div class="marker-handle"></div>
              </div>
              <div v-if="playbackMs !== null" class="play-cursor" :style="{ left: markerLeft(playbackMs) }"></div>
            </template>
          </div>
          <div class="rec-buttons">
            <button type="button" class="mini-btn rec" :disabled="isRecording" @click="onStartRecord">开始录制</button>
            <button type="button" class="mini-btn rec" :disabled="!isRecording" @click="onStopRecord">结束录制</button>
            <button type="button" class="mini-btn" :disabled="isRecording || !hasRecording" @click="onPreviewSelection">试听</button>
            <button type="button" class="mini-btn primary" :disabled="isRecording || !hasRecording" @click="onSaveTrim">保存</button>
            <button type="button" class="mini-btn" @click="onCancelPanel">取消</button>
          </div>
        </div>
      </div>

      <!-- 关闭提示音 -->
      <div class="sound-item">
        <div class="sound-line">
          <span class="dot" :class="panelTarget === 'stop' ? 'dot-rec' : stopExists ? 'dot-on' : 'dot-off'"></span>
          <span class="sound-label">关闭提示音</span>
          <span class="sound-state">
            <template v-if="panelTarget === 'stop'">录制 / 剪裁中</template>
            <template v-else-if="stopExists">已录制</template>
            <template v-else>未录制</template>
          </span>
          <span class="sound-actions" v-if="panelTarget !== 'stop'">
            <button type="button" class="mini-btn" :disabled="!stopExists || panelTarget !== null || simulationBusy" @click="onPreviewFile('stop')">试听</button>
            <button type="button" class="mini-btn rec" :disabled="panelTarget !== null || micUnavailable || simulationBusy" @click="onOpenRecordPanel('stop')">录制</button>
          </span>
        </div>
        <div v-if="panelTarget === 'stop'" ref="recPanelEl" class="rec-panel">
          <span class="rec-status">
            <template v-if="isRecording">录制中 {{ recElapsedLabel }}</template>
            <template v-else-if="hasRecording">{{ trimRangeLabel }}</template>
            <template v-else>点击「开始录制」开始</template>
          </span>
          <div class="wave-wrap">
            <canvas ref="waveCanvas" class="wave" height="60"></canvas>
            <template v-if="!isRecording && hasRecording">
              <div class="trim-marker start" :style="{ left: markerLeft(trimStart) }" @mousedown="onMarkerMouseDown('start', $event)">
                <div class="marker-handle"></div>
              </div>
              <div class="trim-marker end" :style="{ left: markerLeft(trimEnd) }" @mousedown="onMarkerMouseDown('end', $event)">
                <div class="marker-handle"></div>
              </div>
              <div v-if="playbackMs !== null" class="play-cursor" :style="{ left: markerLeft(playbackMs) }"></div>
            </template>
          </div>
          <div class="rec-buttons">
            <button type="button" class="mini-btn rec" :disabled="isRecording" @click="onStartRecord">开始录制</button>
            <button type="button" class="mini-btn rec" :disabled="!isRecording" @click="onStopRecord">结束录制</button>
            <button type="button" class="mini-btn" :disabled="isRecording || !hasRecording" @click="onPreviewSelection">试听</button>
            <button type="button" class="mini-btn primary" :disabled="isRecording || !hasRecording" @click="onSaveTrim">保存</button>
            <button type="button" class="mini-btn" @click="onCancelPanel">取消</button>
          </div>
        </div>
      </div>

      <span v-if="recMessage" class="save-msg sound-msg" :class="{ 'msg-warning': recIsWarning }">{{ recMessage }}</span>
    </div>
  </section>
</template>

<style scoped>
.settings-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 16px 18px;
  gap: 14px;
  overflow-x: hidden;
  overflow-y: auto;
  scrollbar-gutter: stable;
}

/* 滚动条样式 — 同 KeyboardPage / MousePage */
.settings-page::-webkit-scrollbar {
  width: 8px;
}

.settings-page::-webkit-scrollbar-track {
  background: transparent;
}

.settings-page::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 4px;
}

.settings-page::-webkit-scrollbar-thumb:hover {
  background: var(--text-disabled);
}

.page-header {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.page-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0;
}

.page-desc {
  font-size: 11px;
  color: var(--text-disabled);
  margin: 0;
  line-height: 1.4;
}

.form {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 18px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 7px;
}

.form-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.form-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  flex-shrink: 0;
}

.form-footer {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  height: 32px;
  margin-top: 2px;
}

.save-msg {
  font-size: 11px;
  color: var(--success);
  letter-spacing: 0.3px;
  animation: fade-in var(--transition-normal) var(--ease-default);
}

.save-msg.msg-warning {
  color: var(--warning);
}

@keyframes fade-in {
  from {
    opacity: 0;
    transform: translateY(2px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.save-btn {
  height: 32px;
  width: 140px;
  padding: 0;
  border-radius: 6px;
  background: var(--accent);
  color: var(--color-paper-white);
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.5px;
  transition:
    background var(--transition-fast) var(--ease-default),
    opacity var(--transition-fast) var(--ease-default);
}

.save-btn:hover:not(:disabled) {
  background: var(--accent-hover);
}

.save-btn:active:not(:disabled) {
  background: var(--accent-pressed);
}

.save-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.log-control {
  display: flex;
  align-items: center;
  gap: 10px;
}

.log-select {
  width: 140px;
  height: 30px;
  padding: 0 10px;
  color: var(--text-primary);
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: 5px;
}

/* ===== 提示音录制 ===== */
.section-gap {
  margin-top: 4px;
}

.sound-form {
  gap: 10px;
}

.sound-item {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.sound-line {
  display: flex;
  align-items: center;
  gap: 10px;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.dot-on {
  background: var(--success);
}

.dot-off {
  background: var(--text-disabled);
}

.dot-rec {
  background: var(--danger);
  animation: pulse 1s infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.4;
  }
}

.sound-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  flex-shrink: 0;
}

.sound-state {
  font-size: 11px;
  color: var(--text-secondary);
  flex: 1;
}

.sound-actions {
  display: flex;
  gap: 8px;
  width: 140px;
  flex-shrink: 0;
}

.sound-actions .mini-btn {
  flex: 1;
  padding: 0;
}

.mini-btn {
  height: 26px;
  padding: 0 12px;
  border-radius: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 11px;
  transition: background var(--transition-fast) var(--ease-default);
}

.mini-btn:hover:not(:disabled) {
  background: var(--border-color);
}

.mini-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.mini-btn.rec {
  border-color: var(--accent);
  color: var(--accent);
}

.mini-btn.rec:hover:not(:disabled) {
  background: var(--accent);
  color: var(--color-paper-white);
}

.mini-btn.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--color-paper-white);
}

.mini-btn.primary:hover:not(:disabled) {
  background: var(--accent-hover);
}

/* ===== 统一录制 / 剪裁面板 ===== */
.rec-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-left: 18px;
}

.rec-status {
  font-size: 11px;
  color: var(--text-secondary);
}

.wave-wrap {
  position: relative;
  width: 100%;
}

.wave {
  width: 100%;
  height: 60px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 5px;
  display: block;
}

.trim-marker {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--accent);
  cursor: ew-resize;
}

.trim-marker .marker-handle {
  position: absolute;
  top: -4px;
  left: 50%;
  transform: translateX(-50%);
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--accent);
  border: 2px solid var(--color-paper-white);
}

/* 试听进度线 — 与起始 / 结束标记（橙色）区别开（绿色） */
.play-cursor {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--success);
  pointer-events: none;
}

.rec-buttons {
  display: flex;
  gap: 8px;
}

.sound-msg {
  align-self: flex-end;
}
</style>
