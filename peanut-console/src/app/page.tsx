import React from 'react';

export default function Home() {
  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100 p-8 font-sans">
      <header className="mb-12">
        <h1 className="text-3xl font-bold tracking-tight">🥜 Peanut Console</h1>
        <p className="text-neutral-400 mt-2">Minimalist Backend Platform</p>
      </header>
      
      <main className="grid grid-cols-1 md:grid-cols-3 gap-6">
        <div className="bg-neutral-900 p-6 rounded-lg border border-neutral-800 shadow-xl">
          <h2 className="text-lg font-semibold mb-2">Systems</h2>
          <p className="text-3xl font-mono text-green-500">Operational</p>
        </div>
        
        <div className="bg-neutral-900 p-6 rounded-lg border border-neutral-800 shadow-xl">
          <h2 className="text-lg font-semibold mb-2">Storage</h2>
          <p className="text-3xl font-mono">1.2 GB</p>
        </div>
        
        <div className="bg-neutral-900 p-6 rounded-lg border border-neutral-800 shadow-xl">
          <h2 className="text-lg font-semibold mb-2">Push Queue</h2>
          <p className="text-3xl font-mono">0 Pending</p>
        </div>
      </main>
      
      <footer className="mt-12 text-neutral-500 text-sm">
        &copy; 2026 Project Peanut. Single-binary simplicity.
      </footer>
    </div>
  );
}
