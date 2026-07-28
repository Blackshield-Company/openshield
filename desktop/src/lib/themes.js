// OpenShark Desktop — theme engine.
// All colors are CSS vars defined in app.css; a theme just overrides them
// on :root. Selection persists to localStorage.

export const THEMES = [
  {
    id: 'neon-frenzy',
    name: 'Neon Frenzy',
    icon: '🦈',
    desc: 'The default feeding frenzy — hot pink on abyssal black',
    vars: {
      '--bg': '#0a0a12',
      '--bg-panel': '#10101c',
      '--bg-elevated': '#161628',
      '--border': '#26264a',
      '--neon-pink': '#ff2d78',
      '--neon-cyan': '#00e5ff',
      '--neon-purple': '#b967ff',
      '--neon-yellow': '#ffd319',
      '--text': '#e6e6f0',
      '--text-dim': '#8888a8',
      '--success': '#2dff8f',
      '--error': '#ff4d5e',
    },
  },
  {
    id: 'synthwave-84',
    name: "Synthwave '84",
    icon: '🌆',
    desc: 'The classic. Deep purple grid, electric violet sunset',
    vars: {
      '--bg': '#160729',
      '--bg-panel': '#1e0b3d',
      '--bg-elevated': '#31176e',
      '--border': '#4b2a8f',
      '--neon-pink': '#ff2d78',
      '--neon-cyan': '#01cdfe',
      '--neon-purple': '#b967ff',
      '--neon-yellow': '#fffb96',
      '--text': '#f0e6ff',
      '--text-dim': '#9d7fd4',
      '--success': '#05ffa1',
      '--error': '#ff4d5e',
    },
  },
  {
    id: 'great-white',
    name: 'Great White',
    icon: '🤍',
    desc: 'Apex predator in broad daylight — clean light mode',
    vars: {
      '--bg': '#eef1f5',
      '--bg-panel': '#ffffff',
      '--bg-elevated': '#e2e8f0',
      '--border': '#c3cedb',
      '--neon-pink': '#d61f69',
      '--neon-cyan': '#0284c7',
      '--neon-purple': '#7c3aed',
      '--neon-yellow': '#b45309',
      '--text': '#1a2433',
      '--text-dim': '#5d6b7f',
      '--success': '#0e9f5c',
      '--error': '#d92d3c',
    },
  },
  {
    id: 'mako',
    name: 'Mako',
    icon: '💨',
    desc: 'Fastest shark in the ocean — chrome blue velocity',
    vars: {
      '--bg': '#060d16',
      '--bg-panel': '#0a1622',
      '--bg-elevated': '#10233a',
      '--border': '#1e3a5c',
      '--neon-pink': '#ff5d8f',
      '--neon-cyan': '#4cc9ff',
      '--neon-purple': '#7aa2ff',
      '--neon-yellow': '#ffd166',
      '--text': '#dceaf7',
      '--text-dim': '#6f8aa5',
      '--success': '#38e8a0',
      '--error': '#ff5a66',
    },
  },
  {
    id: 'hammerhead',
    name: 'Hammerhead',
    icon: '🔨',
    desc: 'Industrial amber on gunmetal — built, not born',
    vars: {
      '--bg': '#100f0c',
      '--bg-panel': '#17150f',
      '--bg-elevated': '#232017',
      '--border': '#3d3626',
      '--neon-pink': '#ff6b4a',
      '--neon-cyan': '#4adfff',
      '--neon-purple': '#c9a0ff',
      '--neon-yellow': '#ffb300',
      '--text': '#f0ead9',
      '--text-dim': '#948a6e',
      '--success': '#7ee787',
      '--error': '#ff4d5e',
    },
  },
  {
    id: 'tiger-shark',
    name: 'Tiger Shark',
    icon: '🐯',
    desc: 'Orange stripes on black water. Eats anything.',
    vars: {
      '--bg': '#0c0805',
      '--bg-panel': '#140d07',
      '--bg-elevated': '#211507',
      '--border': '#40290f',
      '--neon-pink': '#ff4d6d',
      '--neon-cyan': '#2dd4ff',
      '--neon-purple': '#b98cff',
      '--neon-yellow': '#ff8c1a',
      '--text': '#f5ece1',
      '--text-dim': '#a08a6d',
      '--success': '#4ade80',
      '--error': '#ff4d5e',
    },
  },
  {
    id: 'megalodon',
    name: 'Megalodon',
    icon: '🦷',
    desc: 'Prehistoric abyss — teal bioluminescence in crushing dark',
    vars: {
      '--bg': '#020d0d',
      '--bg-panel': '#061716',
      '--bg-elevated': '#0b2423',
      '--border': '#15403d',
      '--neon-pink': '#ff5e7a',
      '--neon-cyan': '#2affd5',
      '--neon-purple': '#64d8ff',
      '--neon-yellow': '#d4ff4d',
      '--text': '#d9f2ee',
      '--text-dim': '#5f8f89',
      '--success': '#2dff8f',
      '--error': '#ff4d5e',
    },
  },
  {
    id: 'blood-in-the-water',
    name: 'Blood in the Water',
    icon: '🩸',
    desc: 'Crimson tide. The frenzy is already here.',
    vars: {
      '--bg': '#120507',
      '--bg-panel': '#1b080c',
      '--bg-elevated': '#2a0d14',
      '--border': '#4d1520',
      '--neon-pink': '#ff3355',
      '--neon-cyan': '#4dd7ff',
      '--neon-purple': '#c084fc',
      '--neon-yellow': '#ffb454',
      '--text': '#f5e3e6',
      '--text-dim': '#a06a74',
      '--success': '#3ddc84',
      '--error': '#ff4d5e',
    },
  },
  {
    id: 'reef',
    name: 'Reef',
    icon: '🪸',
    desc: 'Coral shallows — tropical teal and living coral',
    vars: {
      '--bg': '#041418',
      '--bg-panel': '#07222a',
      '--bg-elevated': '#0c323d',
      '--border': '#175262',
      '--neon-pink': '#ff6f91',
      '--neon-cyan': '#3ee6d8',
      '--neon-purple': '#8ecdf5',
      '--neon-yellow': '#ffc75f',
      '--text': '#e0f4f2',
      '--text-dim': '#6ba3ab',
      '--success': '#4ade80',
      '--error': '#ff5a66',
    },
  },
  {
    id: 'shark-week',
    name: 'Shark Week',
    icon: '📺',
    desc: 'Documentary navy — breaching great white, red title card',
    vars: {
      '--bg': '#081226',
      '--bg-panel': '#0d1b36',
      '--bg-elevated': '#15294d',
      '--border': '#274374',
      '--neon-pink': '#ef2d3c',
      '--neon-cyan': '#56ccf2',
      '--neon-purple': '#9b8cff',
      '--neon-yellow': '#f2d024',
      '--text': '#e8eefc',
      '--text-dim': '#7d93bd',
      '--success': '#3ddc84',
      '--error': '#ff4d5e',
    },
  },
  {
    id: 'nurse-shark',
    name: 'Nurse Shark',
    icon: '🏖️',
    desc: 'Bottom-dweller calm — warm sand, zero urgency',
    vars: {
      '--bg': '#131110',
      '--bg-panel': '#1b1815',
      '--bg-elevated': '#262119',
      '--border': '#3f3627',
      '--neon-pink': '#e88ca0',
      '--neon-cyan': '#8ecfc9',
      '--neon-purple': '#bda6d9',
      '--neon-yellow': '#d9b380',
      '--text': '#e8e0d4',
      '--text-dim': '#94887a',
      '--success': '#9dc88d',
      '--error': '#e07a7a',
    },
  },
  {
    id: 'amity-75',
    name: "Amity '75",
    icon: '🎬',
    desc: 'You\'re gonna need a bigger boat. JAWS-poster midnight.',
    vars: {
      '--bg': '#030b1a',
      '--bg-panel': '#071427',
      '--bg-elevated': '#0d2038',
      '--border': '#1b3a5c',
      '--neon-pink': '#e32636',
      '--neon-cyan': '#41c7e8',
      '--neon-purple': '#7d9ff5',
      '--neon-yellow': '#f5e663',
      '--text': '#e2ecf7',
      '--text-dim': '#66809e',
      '--success': '#3ddc84',
      '--error': '#ff4d5e',
    },
  },
];

const STORAGE_KEY = 'openshark-theme';
const DEFAULT_THEME = 'neon-frenzy';

export function getTheme(id) {
  return THEMES.find((t) => t.id === id) ?? THEMES.find((t) => t.id === DEFAULT_THEME);
}

export function applyTheme(id) {
  const theme = getTheme(id);
  for (const [key, value] of Object.entries(theme.vars)) {
    document.documentElement.style.setProperty(key, value);
  }
  document.documentElement.dataset.theme = theme.id;
  localStorage.setItem(STORAGE_KEY, theme.id);
  return theme;
}

export function initTheme() {
  const saved = localStorage.getItem(STORAGE_KEY);
  return applyTheme(saved ?? DEFAULT_THEME);
}

export function currentThemeId() {
  return document.documentElement.dataset.theme ?? DEFAULT_THEME;
}
