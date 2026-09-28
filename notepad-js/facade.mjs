import native from './index.js'

export const Notepad = native.Notepad
export const Synthesizer = native.Synthesizer
export const version = native.version
export const isNotepadStub = native.isNotepadStub
export const synthesisUserPrompt = native.synthesisUserPrompt
export const offloadConfigFromObject = native.offloadConfigFromObject
export const offloadConfigToObject = native.offloadConfigToObject

/** @typedef {import('./index.d.ts').OffloadConfig} OffloadConfig */

/**
 * Register notepad scratchpad tools on a superglue Client.
 * @param {import('superglue-js').Client} client
 * @param {InstanceType<typeof Notepad>} notepad
 */
export async function registerTools(client, notepad) {
  native.bridgeRegisterTools(client.toolsRegistryPtr(), notepad.nativeHandle())
}

/**
 * Register automatic tool-result offload hook on a superglue Client.
 * @param {import('superglue-js').Client} client
 * @param {InstanceType<typeof Notepad>} notepad
 * @param {import('./index.d.ts').OffloadConfig} [config]
 */
export async function registerOffloadHook(client, notepad, config) {
  native.bridgeRegisterOffloadHook(
    client.hooksRegistryPtr(),
    notepad.nativeHandle(),
    config ?? null,
  )
}

/**
 * Stub large tool-role messages, storing payloads in notepad.
 * @param {any[]} messages
 * @param {InstanceType<typeof Notepad>} notepad
 * @param {import('./index.d.ts').OffloadConfig} [config]
 */
export function compactToolMessages(messages, notepad, config) {
  return native.bridgeCompactToolMessages(messages, notepad.nativeHandle(), config ?? null)
}

export default {
  Notepad,
  Synthesizer,
  version,
  isNotepadStub,
  synthesisUserPrompt,
  offloadConfigFromObject,
  offloadConfigToObject,
  registerTools,
  registerOffloadHook,
  compactToolMessages,
}
