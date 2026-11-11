// Keep Graphviz geometry separate from state overlays so refreshes do not move nodes.
const fs = require('node:fs');
const { instance } = require('@viz-js/viz');
const { Resvg } = require('@resvg/resvg-js');
async function main() {
  const [mode, input, output] = process.argv.slice(2);
  const source = fs.readFileSync(input, 'utf8');
  if (mode === 'layout') {
    const viz = await instance();
    fs.writeFileSync(output, viz.renderString(source, { format: 'svg', engine: 'dot' }));
  } else if (mode === 'png') {
    fs.writeFileSync(output, new Resvg(source, {
      fitTo: { mode: 'zoom', value: 2 }, font: { loadSystemFonts: true }
    }).render().asPng());
  } else {
    throw new Error('Expected layout or png mode');
  }
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
