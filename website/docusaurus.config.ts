import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';
import {themes as prismThemes} from 'prism-react-renderer';

// URL e baseUrl configuraveis por ambiente, para que um futuro
// garraia.org/docs/ ou docs.garraia.org nao precise de mudanca de codigo.
// Default: GitHub Pages deste repositorio.
const siteUrl = process.env.DOCS_SITE_URL ?? 'https://michelbr84.github.io';
const baseUrl = process.env.DOCS_BASE_URL ?? '/GarraRUST/';

const githubRepo = 'https://github.com/michelbr84/GarraRUST';

const config: Config = {
  title: 'GarraIA',
  // Tagline tirada do README (sem inventar).
  tagline:
    'The secure, lightweight open-source framework for AI agents — written in Rust, born in Brazil.',
  favicon: 'img/favicon.ico',

  url: siteUrl,
  baseUrl,

  organizationName: 'michelbr84',
  projectName: 'GarraRUST',

  // Os arquivos-fonte nao foram escritos para MDX (contem `<`, `{}` e HTML
  // solto). Links quebrados ficam como aviso, nunca falham o build.
  onBrokenLinks: 'warn',
  onBrokenMarkdownLinks: 'warn',
  onBrokenAnchors: 'warn',

  markdown: {
    // `.md` tratado como CommonMark (nao MDX) — e o que evita o build
    // quebrar nos arquivos existentes.
    format: 'detect',
  },

  i18n: {
    defaultLocale: 'pt-BR',
    locales: ['pt-BR', 'en'],
    localeConfigs: {
      'pt-BR': {label: 'Português (Brasil)', htmlLang: 'pt-BR'},
      en: {label: 'English', htmlLang: 'en'},
    },
  },

  presets: [
    [
      'classic',
      {
        docs: {
          path: 'docs',
          routeBasePath: '/',
          sidebarPath: './sidebars.ts',
          // O edit URL real fica em `custom_edit_url` (o sync-docs grava o
          // caminho do arquivo-fonte), nao no caminho gerado.
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themes: [
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      {
        hashed: true,
        language: ['en', 'pt'],
        indexDocs: true,
        indexBlog: false,
        docsRouteBasePath: '/',
        highlightSearchTermsOnTargetPage: true,
        explicitSearchResultPath: true,
      },
    ],
  ],

  themeConfig: {
    image: 'img/logo.svg',
    colorMode: {
      defaultMode: 'light',
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'GarraIA',
      logo: {
        alt: 'GarraIA',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {to: '/changelog', label: 'Changelog', position: 'left'},
        {to: '/releases', label: 'Releases', position: 'left'},
        {type: 'localeDropdown', position: 'right'},
        {href: githubRepo, label: 'GitHub', position: 'right'},
        {href: 'https://garraia.org', label: 'Site', position: 'right'},
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'GarraIA',
          items: [
            {label: 'Repositório', href: githubRepo},
            {label: 'Issues', href: `${githubRepo}/issues`},
            {label: 'Releases', href: `${githubRepo}/releases`},
            {label: 'Wiki', href: `${githubRepo}/wiki`},
            {label: 'Site', href: 'https://garraia.org'},
          ],
        },
      ],
      copyright: `GarraIA — projeto de código aberto (MIT). Documentação gerada a partir do repositório.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: [
        'rust',
        'bash',
        'toml',
        'json',
        'yaml',
        'powershell',
        'docker',
        'sql',
      ],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
