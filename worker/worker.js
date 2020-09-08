addEventListener('fetch', event => {
  event.respondWith(handle(event))
})

async function handle(event) {
  const { main } = wasm_bindgen;
  await wasm_bindgen(wasm)
  return await main(event)
}
