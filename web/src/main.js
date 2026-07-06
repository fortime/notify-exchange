import { createApp } from 'vue'
import App from './App.vue'
import router from './router'

// Import Bootstrap and Bootstrap-Vue-Next CSS files
import 'bootstrap/dist/css/bootstrap.css'
import 'bootstrap-vue-next/dist/bootstrap-vue-next.css'
import 'bootstrap-icons/font/bootstrap-icons.css'

import VueToast from 'vue-toast-notification'
import 'vue-toast-notification/dist/theme-sugar.css'

const app = createApp(App)
app.use(router)
app.use(VueToast)
app.mount('#app')
