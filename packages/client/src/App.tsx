import { useEffect } from 'react';
import { useAppStore } from './lib/store';
import TitleBar from './components/TitleBar';
import Sidebar from './components/Sidebar';
import SearchPalette from './components/SearchPalette';
import HomeView from './views/HomeView';
import AutomationsView from './views/AutomationsView';
import CustomizeView from './views/CustomizeView';
import AgentView from './views/AgentView';
import EditorView from './views/EditorView';
import SettingsView from './views/SettingsView';

export default function App() {
  const route = useAppStore((s) => s.route);
  const sidebarVisible = useAppStore((s) => s.sidebarVisible);
  const setPaletteOpen = useAppStore((s) => s.setPaletteOpen);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [setPaletteOpen]);

  return (
    <div className="flex h-full flex-col bg-white">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        {sidebarVisible && <Sidebar />}
        <main className="min-w-0 flex-1">
          {route === 'home' && <HomeView />}
          {route === 'automations' && <AutomationsView />}
          {route === 'customize' && <CustomizeView />}
          {route === 'agent' && <AgentView />}
          {route === 'editor' && <EditorView />}
          {route === 'settings' && <SettingsView />}
        </main>
      </div>
      <SearchPalette />
    </div>
  );
}
