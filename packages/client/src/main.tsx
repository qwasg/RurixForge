import React from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/dm-sans';
import '@fontsource-variable/jetbrains-mono';
import './styles/index.css';
import App from './App';

const container = document.getElementById('root');
if (!container) throw new Error('missing #root');

createRoot(container).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
