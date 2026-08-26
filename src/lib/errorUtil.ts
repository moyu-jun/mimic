export type ErrorRecoveryPolicy = 'criticalRuntime' | 'localOperation' | 'optionalAudio'

export interface CommandErrorDto {
  code: string
  message: string
  retryable: boolean
  recovery: ErrorRecoveryPolicy
}

function isCommandErrorDto(value: unknown): value is CommandErrorDto {
  if (!value || typeof value !== 'object') return false
  const candidate = value as Partial<CommandErrorDto>
  return (
    typeof candidate.code === 'string' &&
    typeof candidate.message === 'string' &&
    typeof candidate.retryable === 'boolean' &&
    typeof candidate.recovery === 'string'
  )
}

export function commandError(error: unknown): CommandErrorDto {
  if (isCommandErrorDto(error)) return error

  if (typeof error === 'string') {
    try {
      const parsed: unknown = JSON.parse(error)
      if (isCommandErrorDto(parsed)) return parsed
    } catch {
      // Tauri 旧版或非 command 错误可能只是普通字符串。
    }
    const prefix = error.split(/[:\s]/, 1)[0]
    return {
      code: /^[a-z0-9_]{1,64}$/.test(prefix) ? prefix : 'operation_failed',
      message: error,
      retryable: true,
      recovery: 'localOperation',
    }
  }

  if (error instanceof Error) {
    return {
      code: 'operation_failed',
      message: error.message,
      retryable: true,
      recovery: 'localOperation',
    }
  }

  return {
    code: 'operation_failed',
    message: 'Unknown operation error',
    retryable: true,
    recovery: 'localOperation',
  }
}

export function commandErrorMessage(error: unknown): string {
  return commandError(error).message
}