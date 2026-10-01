// One-time AST migration: only source literals are translated, never runtime user content.
// Emits an apply_patch patch; does not write source files.
import ts from "typescript";
import fs from "node:fs";
import path from "node:path";
let patch = "*** Begin Patch\n";
for (const file of fs.readdirSync("src",{recursive:true}).filter(file => /\.tsx?$/.test(file) && !file.startsWith("i18n/") && !file.endsWith(".test.ts"))) {
  if (process.argv[2] && file !== process.argv[2]) continue;
  const source = fs.readFileSync(`src/${file}`,"utf8");
  const ast = ts.createSourceFile(file,source,ts.ScriptTarget.Latest,true,file.endsWith("tsx")?ts.ScriptKind.TSX:ts.ScriptKind.TS);
  function replacements(root) {
    const edits=[];
    function visit(node) {
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "ui") return;
      if (ts.isTemplateExpression(node)) {
        const key = node.head.text + node.templateSpans.map((span,index) => `{p${index}}${span.literal.text}`).join("");
        if (/[\u3400-\u9fff]/.test(key)) {
          const params = node.templateSpans.map((span,index) => {
            let value = span.expression.getText(ast);
            const offset = span.expression.getStart(ast);
            for(const edit of replacements(span.expression).reverse()) value = value.slice(0,edit.start-offset)+edit.value+value.slice(edit.end-offset);
            return `p${index}: String(${value})`;
          }).join(", ");
          edits.push({start:node.getStart(ast),end:node.end,value:`ui(${JSON.stringify(key)}, { ${params} })`}); return;
        }
      }
      if ((ts.isStringLiteral(node)||ts.isNoSubstitutionTemplateLiteral(node)||ts.isJsxText(node)) && /[\u3400-\u9fff]/.test(node.text)) {
        // Backend error matching is protocol handling, not displayed copy.
        if (ts.isCallExpression(node.parent) && ts.isPropertyAccessExpression(node.parent.expression) && node.parent.expression.name.text === "includes") return;
        const key=node.text.trim();
        const jsx=ts.isJsxText(node)||ts.isJsxAttribute(node.parent);
        edits.push({start:ts.isJsxText(node)?node.pos:node.getStart(ast),end:node.end,value:(jsx?"{":"")+`ui(${JSON.stringify(key)})`+(jsx?"}":"")}); return;
      }
      ts.forEachChild(node,visit);
    }
    visit(root); return edits.sort((a,b)=>a.start-b.start);
  }
  const edits=replacements(ast);
  if(!edits.length) continue;
  let next=source;
  for(const edit of edits.reverse()) next=next.slice(0,edit.start)+edit.value+next.slice(edit.end);
  const relative=path.relative(path.dirname(`src/${file}`),"src/i18n/ui").replaceAll("\\","/");
  next=`import { ui } from ${JSON.stringify(relative.startsWith('.')?relative:'./'+relative)};\n`+next;
  patch+=`*** Update File: src/${file}\n@@\n`+source.split('\n').map(line=>'-'+line).join('\n')+'\n'+next.split('\n').map(line=>'+'+line).join('\n')+'\n';
}
process.stdout.write(patch+"*** End Patch\n");
