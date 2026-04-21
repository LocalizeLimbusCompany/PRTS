const fs = require('fs');

const path = 'web/src/features/translation/components/TranslationTable.tsx';
let content = fs.readFileSync(path, 'utf8');

content = content.replace(/className="w-8 p-2/g, 'className="w-8 px-2 py-1');
content = content.replace(/className="w-1\/3 p-2/g, 'className="w-1/3 px-2 py-1');
content = content.replace(/className="w-1\/2 p-2/g, 'className="w-1/2 px-2 py-1');
content = content.replace(/className="w-32 p-2/g, 'className="w-32 px-2 py-1');
content = content.replace(/<td className="p-2 pl-3">/g, '<td className="px-2 py-1 pl-3">');
content = content.replace(/<td className="p-2 border-r/g, '<td className="px-2 py-1 border-r');
content = content.replace(/<td className="p-2 text-xs">/g, '<td className="px-2 py-1 text-xs">');

fs.writeFileSync(path, content, 'utf8');