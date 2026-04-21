const fs = require('fs');
const path = require('path');

const replacements = [
  { regex: /min-h-\[240px\]/g, replace: 'min-h-[120px]' },
  { regex: /min-h-\[160px\]/g, replace: 'min-h-[80px]' },
  { regex: /w-\[24rem\]/g, replace: 'w-[18rem]' },
  { regex: /w-\[20rem\]/g, replace: 'w-[16rem]' },
  { regex: /w-\[300px\]/g, replace: 'w-[200px]' },
  { regex: /h-16\b/g, replace: 'h-10' },
  { regex: /h-14\b/g, replace: 'h-9' }
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