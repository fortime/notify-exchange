<template>
  <BRow class="justify-content-md-center mt-5">
    <BCol md="6">
      <BCard header-tag="header">
        <template #header>
          <h4 class="mb-0">
            Login
          </h4>
        </template>
        <div v-if="step === 'email'">
          <BCardText>Please enter your email address to begin.</BCardText>
          <BForm @submit.prevent="requestToken">
            <BFormGroup
              id="input-group-email"
              label="Email address:"
              label-for="input-email"
              class="text-start"
            >
              <BFormInput
                id="input-email"
                v-model="email"
                type="email"
                placeholder="your@email.com"
                required
              />
            </BFormGroup>

            <BAlert
              variant="danger"
              :show="errorMessage !== ''"
            >
              {{ errorMessage }}
            </BAlert>

            <BButton
              type="submit"
              variant="primary"
              class="mt-3"
            >
              Login
            </BButton>
          </BForm>
        </div>

        <div v-if="step === 'token'">
          <BCardText>Check Your Telegram</BCardText>
          <p>A login token has been generated. Please send the following command to your Telegram bot:</p>

          <BAlert
            show
            variant="success"
            body-class="d-flex align-items-center container-lg"
          >
            <div class="text-truncate">
              /login {{ token }}
            </div>
            <div class="ms-auto">
              <BButton
                @click="copyCommand"
                size="sm"
                variant="outline-secondary"
              >
                {{ copiedMessage || 'Copy' }}
              </BButton>
            </div>
          </BAlert>

          <p class="text-muted">
            This page will automatically redirect once you confirm in Telegram.
          </p>
          <p class="text-muted small">
            This request will time out in {{ Math.floor(timeout/1000) }} seconds.
          </p>
        </div>
      </BCard>
    </BCol>
  </BRow>
</template>

<script>
import apiClient from "../service/api"
import { state } from "../service/state"
import {
  BRow,
  BCol,
  BCard,
  BCardText,
  BForm,
  BFormGroup,
  BFormInput,
  BButton,
  BAlert
} from 'bootstrap-vue-next'

const POLLING_INTERVAL = 3000 // 3 seconds
const POLLING_TIMEOUT = 300000 // 5 minutes

export default {
  name: "AppLogin",
  components: {
    BRow,
    BCol,
    BCard,
    BCardText,
    BForm,
    BFormGroup,
    BFormInput,
    BButton,
    BAlert
  },
  data() {
    return {
      step: "email", // 'email' or 'token'
      email: "",
      token: null,
      ru: null,
      pollingId: null,
      timeoutId: null,
      timeout: POLLING_TIMEOUT,
      errorMessage: "",
      copiedMessage: "",
    }
  },
  mounted() {
    this.ru = this.$route.query.ru || "/home"
    if (state.user) {
      this.$router.push(this.ru)
    }
  },
  beforeUnmount() {
    this.stopPolling()
  },
  methods: {
    async requestToken() {
      this.errorMessage = ""
      try {
        const response = await apiClient.post("/v1/user/session", { email: this.email })
        this.token = response.data.data.token
        this.step = "token"
        this.startPolling()
      } catch (error) {
        console.error("Failed to request token:", error)
        this.errorMessage =
          error.response?.data?.message || "Failed to request token. Please try again."
      }
    },
    startPolling() {
      if (this.pollingId) return

      this.pollingId = setInterval(async () => {
        try {
          const response = await apiClient.get("/v1/user/session", {
            params: { token: this.token },
          })
          if (response.data.data.csrf_token) {
            // Login confirmed!
            this.stopPolling()
            localStorage.setItem('csrfToken', response.data.data.csrf_token)
            state.isLoggedIn = true
            this.$router.push(this.ru)
          }
        } catch (error) {
          // It's normal to get errors here while polling, so we don't show them
          // unless it's not a 404 or other expected error.
          if(error.response?.status !== 404) {
            console.error("Polling error:", error)
          }
        }
      }, POLLING_INTERVAL)

      // Set a timeout for the whole process
      this.timeoutId = setTimeout(() => {
        this.stopPolling()
        this.errorMessage = "Login request timed out."
        this.step = 'email' // Go back to email step
      }, POLLING_TIMEOUT)

      // Countdown for display
      const timer = setInterval(() => {
        this.timeout -= 1000
        if(this.timeout <= 0) clearInterval(timer)
      }, 1000)
    },
    stopPolling() {
      if (this.pollingId) {
        clearInterval(this.pollingId)
        this.pollingId = null
      }
      if (this.timeoutId) {
        clearTimeout(this.timeoutId)
        this.timeoutId = null
      }
    },
    async copyCommand() {
      const commandText = `/login ${this.token}`
      try {
        await navigator.clipboard.writeText(commandText)
        this.copiedMessage = 'Copied!'
        setTimeout(() => {
          this.copiedMessage = ''
        }, 2000)
      } catch (err) {
        console.error('Failed to copy text:', err)
        this.copiedMessage = 'Failed to copy!'
        setTimeout(() => {
          this.copiedMessage = ''
        }, 2000)
      }
    },
  },
}
</script>
