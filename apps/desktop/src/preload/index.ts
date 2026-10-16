import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";

// Kept dependency-free on purpose: channel/payload validation happens with Schema on both ends.
const api = {
  invoke: (channel: string, payload: unknown): Promise<unknown> =>
    ipcRenderer.invoke(`ss:${channel}`, payload),
  on: (channel: string, listener: (payload: unknown) => void): (() => void) => {
    const handler = (_event: IpcRendererEvent, payload: unknown) => listener(payload);
    ipcRenderer.on(`ss:${channel}`, handler);
    return () => ipcRenderer.removeListener(`ss:${channel}`, handler);
  },
};

contextBridge.exposeInMainWorld("ss", api);

export type SsBridge = typeof api;
