interface Window {
  redcrown: {
    invoke<T>(method: string, params?: Record<string, unknown>): Promise<T>;
    windowControls: {
      minimize(): Promise<boolean>;
      toggleMaximize(): Promise<boolean>;
      close(): Promise<boolean>;
      isMaximized(): Promise<boolean>;
      onMaximized(callback: (maximized: boolean) => void): () => void;
    };
  };
}
