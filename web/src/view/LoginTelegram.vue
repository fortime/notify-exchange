<template>
  <BRow class="justify-content-md-center mt-5">
    <BCol md="6">
      <BCard
        header="Telegram Login"
        class="text-center"
      >
        <div class="d-flex justify-content-center align-items-center p-5">
          <BSpinner label="Spinning" />
          <p class="ms-3 mb-0">
            {{ message }}
          </p>
        </div>
      </BCard>
    </BCol>
  </BRow>
</template>

<script>
import api from "../service/api"
import { state } from '../service/state'
import {
  BRow,
  BCol,
  BCard,
  BSpinner
} from 'bootstrap-vue-next'


export default {
  name: "LoginTelegram",
  components: {
    BRow,
    BCol,
    BCard,
    BSpinner
  },
  data() {
    return {
      message: "Logging in...",
    }
  },
  mounted() {
    const script = document.createElement("script")
    script.src = "https://telegram.org/js/telegram-web-app.js"
    script.onload = () => this.handleSdkLoad()
    script.onerror = () => {
      this.message = "Failed to load Telegram SDK."
    }
    document.head.appendChild(script)
  },
  methods: {
    async handleSdkLoad() {
      try {
        const tg = window.Telegram.WebApp
        tg.ready()

        if (state.user) {
            this.$router.push(this.ru)
            return
        }

        const params = new URLSearchParams(window.location.search)
        const eid = params.get("eid")
        const ru = params.get("ru")

        if (!tg.initData || !eid) {
          this.message = "Invalid login parameters. Missing initData or eid."
          return
        }

        const response = await api.post("/v1/user/session/telegram", {
          init_data: tg.initData,
          eid: eid,
        })

        // Save the CSRF token to global state
        if (response.data.data.csrf_token) {
          localStorage.setItem('csrfToken', response.data.data.csrf_token)
          localStorage.setItem('logoutRouteName', 'LogoutTelegram')
          state.isLoggedIn = true
        }

        this.$router.push(ru || "/")
      } catch (error) {
        console.error("Login failed:", error)
        this.message = "Login failed. Please try again later."
        if (error.response && error.response.data && error.response.data.message) {
            this.message = `Login failed: ${error.response.data.message}`
        }
      }
    },
  },
}
</script>
