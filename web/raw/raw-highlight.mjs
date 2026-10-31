import hljs from 'highlight.js/lib/common';
import powershell from 'highlight.js/lib/languages/powershell';
import excel from 'highlight.js/lib/languages/excel';
import http from 'highlight.js/lib/languages/http';
import nginx from 'highlight.js/lib/languages/nginx';
import cmake from 'highlight.js/lib/languages/cmake';
import latex from 'highlight.js/lib/languages/latex';
import dockerfile from 'highlight.js/lib/languages/dockerfile';

hljs.registerLanguage('http', http);
hljs.registerLanguage('nginx', nginx);
hljs.registerLanguage('cmake', cmake);
hljs.registerLanguage('latex', latex);
hljs.registerLanguage('dockerfile', dockerfile);
hljs.registerLanguage('powershell', powershell);
// Excel formula syntax only; binary workbooks are handled as file assets.
hljs.registerLanguage('excel', excel);
hljs.registerLanguage('bibtex', hljs => ({
  name: 'BibTeX',
  aliases: ['bib'],
  contains: [
    hljs.COMMENT(/@comment\s*[{(]/i, /[})]/),
    {
      begin: /@[a-z]+\s*[{(]/i,
      beginScope: 'keyword',
      end: /[})]/,
      contains: [
        { className: 'title', begin: /[\w:./-]+(?=\s*,)/ },
        { className: 'attr', begin: /\b[a-z][\w-]*(?=\s*=)/i },
        { className: 'string', begin: /\{/, end: /\}/, contains: ['self'] },
        hljs.QUOTE_STRING_MODE,
        { className: 'number', begin: /\b\d+\b/ },
      ],
    },
  ],
}));

const languages = {
  typ: 'typst', md: 'markdown', bib: 'bibtex', pwsh: 'powershell',
  tex: 'latex', docker: 'dockerfile',
};
hljs.registerLanguage('typst', hljs => ({
  name: 'Typst',
  aliases: ['typ'],
  contains: [
    hljs.COMMENT('//', '$'),
    { className: 'comment', begin: /\/\*/, end: /\*\//, contains: ['self'] },
    hljs.QUOTE_STRING_MODE,
    { className: 'string', begin: /`/, end: /`/ },
    { className: 'section', begin: /^={1,6}(?=\s)/, end: /$/ },
    { className: 'keyword', begin: /#(?:let|set|show|if|else|for|while|break|continue|return|import|include|as|in|and|or|not)\b/ },
    { className: 'keyword', begin: /\b(?:let|set|show|if|else|for|while|break|continue|return|import|include|as|in|and|or|not)\b/ },
    { className: 'literal', begin: /\b(?:true|false|none|auto)\b/ },
    { className: 'number', begin: /\b\d+(?:\.\d+)?(?:pt|mm|cm|in|em|fr|deg|rad|%)?\b/ },
    { className: 'symbol', begin: /@[\w-]+/ },
    { className: 'tag', begin: /<[\w:-]+>/ },
    { className: 'title.function_', begin: /[\w-]+(?=\s*\()/ },
    { className: 'meta', begin: /^\s*[-+\/](?=\s)/ },
  ],
}));


export function rawLanguage(name) {
  const extension = name.split('.').pop()?.toLowerCase();
  const language = languages[extension] || extension;
  return language && hljs.getLanguage(language) ? language : null;
}

export function highlightElement(element) {
  const className = [...element.classList].find(name => name.startsWith('language-'));
  const language = className ? className.slice('language-'.length) : '';
  if (!language || !hljs.getLanguage(languages[language] || language)) return false;
  element.innerHTML = highlightSource(element.textContent, languages[language] || language);
  element.classList.add('hljs');
  element.dataset.highlighted = 'yes';
  return true;
}

export function highlightSource(source, language) {
  if (language === 'http' || language === 'https') {
    const separator = /\r?\n\r?\n/.exec(source);
    const contentType = separator && source.slice(0, separator.index)
      .match(/^content-type\s*:\s*([^;\s]+)/im)?.[1];
    if (contentType && /^application\/(?:[\w.-]+\+)?json$/i.test(contentType)) {
      const bodyStart = separator.index + separator[0].length;
      return hljs.highlight(source.slice(0, bodyStart), { language: 'http', ignoreIllegals: true }).value
        + hljs.highlight(source.slice(bodyStart), { language: 'json', ignoreIllegals: true }).value;
    }
  }
  return hljs.highlight(source, { language, ignoreIllegals: true }).value;
}

export function highlightRaw(source, name) {
  const language = rawLanguage(name);
  return language ? highlightSource(source, language) : hljs.highlightAuto(source).value;
}

export { hljs };
