declare global {
  interface Window {
    ss: {
      invoke(channel: string, payload: unknown): Promise<unknown>;
      on(channel: string, listener: (payload: unknown) => void): () => void;
    };
  }
}

export {};
