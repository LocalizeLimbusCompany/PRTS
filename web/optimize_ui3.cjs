const fs = require('fs');
const path = require('path');

const replacements = [
  { regex: /\bpx-6\b/g, replace: 'px-3' },
  { regex: /\bpx-8\b/g, replace: 'px-4' },
  { regex: /\bp-4\b/g, replace: 'p-2' },
  { regex: /\bp-6\b/g, replace: 'p-3' },
  { regex: /\bpy-6\b/g, replace: 'py-2' }
];

function processDir(dir) {
  const files = fs.readdirSync(dir);
  for (const file of files) {
    const fullPath = path.join(dir, file);
    if (fs.statSync(fullPath).isDirectory()) {
      processDir(fullPath);
    } else if (fullPath.endsWith('.tsx') || fullPath.endsWith('.ts')) {
      let content = fs.readFileSync(fullPath, 'utf8');
      let newContent = content;
      for (const rule of replacements) {
        newContent = newContent.replace(rule.regex, rule.replace);
      }
      
      if (content !== newContent) {
        fs.writeFileSync(fullPath, newContent, 'utf8');
        console.log(`Updated ${fullPath}`);
      }
    }
  }
}

processDir('./web/src');