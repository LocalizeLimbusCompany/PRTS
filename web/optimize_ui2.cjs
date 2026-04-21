const fs = require('fs');
const path = require('path');

const replacements = [
  { regex: /\bp-12\b/g, replace: 'p-4' },
  { regex: /\bp-10\b/g, replace: 'p-4' },
  { regex: /\bp-8\b/g, replace: 'p-3' },
  { regex: /\bpx-8\b/g, replace: 'px-4' },
  { regex: /\bpy-8\b/g, replace: 'py-3' },
  { regex: /\bpy-10\b/g, replace: 'py-4' },
  { regex: /\bpx-10\b/g, replace: 'px-4' },
  { regex: /\bpx-5\b/g, replace: 'px-3' },
  { regex: /\bpy-5\b/g, replace: 'py-2' },
  { regex: /\bpx-4\b/g, replace: 'px-2' },
  { regex: /\bpy-4\b/g, replace: 'py-2' },
  { regex: /\bspace-y-8\b/g, replace: 'space-y-3' },
  { regex: /\bgap-8\b/g, replace: 'gap-3' },
  { regex: /\bmb-8\b/g, replace: 'mb-3' },
  { regex: /\bmt-8\b/g, replace: 'mt-3' }
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