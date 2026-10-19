import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // The core's worker (src/api/worker.ts) imports the WebAssembly module as an
  // ES module, which the default classic-worker format cannot bundle.
  worker: { format: 'es' },
});
