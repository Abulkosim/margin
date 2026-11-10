import { createApp } from 'vue'
import App from './App.vue'
import './design/fonts.css'
import './styles.css'
import './design/controls.css'

// Mount after Inter loads so the first popover fit is measured with final text metrics.
const fontLoaded = document.fonts.load('14px "Inter Variable"').catch(() => [])
const fallback = new Promise(resolve => setTimeout(resolve, 1000))
void Promise.race([fontLoaded, fallback]).then(() => createApp(App).mount('#app'))
