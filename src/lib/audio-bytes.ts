export type BinaryIpcResponse = ArrayBuffer | ArrayBufferView | number[];

/** Normalizes Tauri raw IPC responses into a Blob-safe byte view. */
export function normalizeAudioBytes(value: BinaryIpcResponse): Uint8Array<ArrayBuffer> {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    return Uint8Array.from(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
  }
  if (Array.isArray(value) && value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)) {
    return Uint8Array.from(value);
  }
  throw new TypeError("Resposta de áudio inválida recebida do backend.");
}
