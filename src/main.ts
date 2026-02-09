import './styles.css';
import App from './App.svelte';
import Hud from './Hud.svelte';

const params = new URLSearchParams(window.location.search);
const Root = params.has('hud') ? Hud : App;

const app = new Root({
  target: document.getElementById('app') as HTMLElement,
});

export default app;
