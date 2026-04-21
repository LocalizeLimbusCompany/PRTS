const fs = require('fs');
const path = require('path');

const regexes = [
  /p-4/, /p-5/, /p-6/, /p-8/, /p-10/,
  /py-4/, /py-5/, /py-6/, /py-8/,
  /px-4/, /px-5/, /px-6/, /px-8/,
  /gap-4/, /gap-5/, /gap-6/, /gap-8/,
  /space-y-4/, /space-y-5/, /space-y-6/,
  /w-80/
];

function processDir(dir) {
  const files = fs.readdirSync(dir);
  for (const file of files) {
    const fullPath = path.join(dir, file);
    if (fs.statSync(fullPath).isDirectory()) {
      processDir(fullPath);
    } else if (fullPath.endsWith('.tsx') || fullPath.endsWith('.ts')) {
      let content = fs.readFileSync(fullPath, 'utf8');
      for (const regex of regexes) {
        if (regex.test(content)) {
          console.log(`Found ${regex} in ${fullPath}`);
          break; // break the regex loop, move to next file
        }
      }
    }
  }
}

processDir('./web/src');