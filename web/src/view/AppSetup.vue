<template>
  <BRow class="justify-content-md-center mt-5">
    <BCol md="8">
      <BCard
        header="System Setup"
        header-tag="header"
      >
        <template #header>
          <h1 class="mb-0">
            System Setup
          </h1>
        </template>

        <BForm
          @submit.prevent="submitSetup"
          v-if="!setupComplete"
        >
          <h2>Create Admin User</h2>
          <BFormGroup
            label="User Name"
            label-for="name"
            class="mb-3 text-start"
          >
            <BFormInput
              id="name"
              v-model="adminUser.name"
              required
            />
          </BFormGroup>
          <BFormGroup
            label="Email"
            label-for="email"
            class="mb-3 text-start"
          >
            <BFormInput
              id="email"
              type="email"
              v-model="adminUser.email"
              required
            />
          </BFormGroup>

          <h2 class="mt-4">
            Select Service Provider
          </h2>
          <BFormGroup
            label="Service Provider"
            label-for="provider"
            class="mb-3 text-start"
          >
            <BFormSelect
              id="provider"
              v-model="selectedProviderId"
              :options="providerOptions"
            />
          </BFormGroup>

          <div
            v-if="selectedProvider"
            class="provider-options mt-4 pt-4 border-top"
          >
            <h3>Options for {{ selectedProvider.name }}</h3>
            <div
              v-for="field in selectedProvider.fields"
              :key="field.name"
              class="mb-3"
            >
              <BFormGroup
                :label="field.label"
                :label-for="field.name"
                class="text-start"
              >
                <BFormInput
                  :id="field.name"
                  :type="field.type"
                  v-model="providerData[field.name]"
                  :required="field.required"
                />
              </BFormGroup>
            </div>
          </div>

          <div class="d-grid mt-4">
            <BButton
              type="submit"
              variant="primary"
              :disabled="!selectedProviderId"
            >
              Submit
            </BButton>
          </div>
        </BForm>

        <div
          v-if="setupComplete"
        >
          <div v-if="selectedProviderId === 1">
            <!-- Telegram -->
            <h2 class="mb-3">
              Nearly There!
            </h2>
            <p class="lead">
              Your system is almost set up. To complete the process, please send the following registration token to your Telegram bot:
            </p>
            <strong>Your Registration Token:</strong>
            <BAlert
              show
              variant="success"
              body-class="d-flex align-items-center container-lg"
            >
              <div class="text-truncate">
                {{ setupResult.token }}
              </div>
              <div class="ms-auto">
                <BButton
                  size="sm"
                  variant="outline-secondary"
                  @click="copyToClipboard(setupResult.token, 'setup-token')"
                >
                  <i class="bi bi-clipboard" />
                </BButton>
                <span v-if="copied === `setup-token`">Copied!</span>
              </div>
            </BAlert>
            <p>After sending the token, your bot will be connected to the system. You can then log in to start using the service.</p>
            <BButton
              variant="success"
              size="lg"
              @click="$router.push({ name: 'Home' })"
            >
              Go to Homepage
            </BButton>
          </div>
          <div v-else>
            <!-- Generic message for other types -->
            <h2>Setup successful!</h2>
            <p class="lead">
              Your system has been set up successfully.
            </p>
            <BButton
              variant="success"
              size="lg"
              @click="$router.push({ name: 'Home' })"
            >
              Go to Homepage
            </BButton>
          </div>
        </div>
      </BCard>
    </BCol>
  </BRow>
</template>

<script>
import {
  BRow,
  BCol,
  BCard,
  BForm,
  BFormGroup,
  BFormInput,
  BFormSelect,
  BButton,
  BAlert,
} from 'bootstrap-vue-next'
import apiClient from '../service/api'
import { TransportServiceType } from '../service/enum'

export default {
  name: 'AppSetup',
  components: {
    BRow,
    BCol,
    BCard,
    BForm,
    BFormGroup,
    BFormInput,
    BFormSelect,
    BButton,
    BAlert,
  },
  data() {
    return {
      adminUser: {
        name: '',
        email: ''
      },
      serviceProviders: [],
      selectedProviderId: null,
      providerData: {},
      setupComplete: false,
      setupResult: null,
      copied: null
    }
  },
  computed: {
    providerOptions() {
      const options = [{ value: null, text: '-- Please select a provider --', disabled: true }]
      this.serviceProviders.forEach(p => {
        options.push({ value: p.id, text: p.name })
      })
      return options
    },
    selectedProvider() {
      if (!this.selectedProviderId) {
        return null
      }
      return this.serviceProviders.find(p => p.id === this.selectedProviderId)
    }
  },
  watch: {
    selectedProviderId() {
      // Reset provider data when selection changes
      this.providerData = {}
    }
  },
  async created() {
    try {
      const providersResponse = await apiClient.get('/v1/admin/setup/transport-service-type')
      this.serviceProviders = providersResponse.data.data.map(id => ({
        id,
        name: TransportServiceType[id]?.name,
        fields: this.getProviderFields(id),
      }))
    } catch (e) {
      console.error('Error fetching service providers:', e)
      this.$toast.error('Error fetching service providers: ' + (e.response?.data?.message || e.message))
    }
  },
  methods: {
    getProviderFields(id) {
      if (id === 1) { // Telegram
        return [
          { name: 'name', label: 'Service Name', type: 'text', required: true },
          { name: 'token', label: 'Bot Token', type: 'password', required: true },
          { name: 'description', label: 'Description', type: 'text', required: false },
        ]
      }
      return []
    },
    async submitSetup() {
      if (!this.selectedProviderId) {
        this.$toast.error('Please select a service provider.')
        return
      }

      let url = ''
      let payload = {}

      if (this.selectedProviderId === 1) { // Telegram
        url = '/v1/admin/setup/telegram'
        payload = {
          admin: this.adminUser,
          token: this.providerData.token,
          name: this.providerData.name,
          description: this.providerData.description,
        }
      } else {
        this.$toast.error('This provider type is not supported yet.')
        return
      }

      try {
        const response = await apiClient.post(url, payload)
        this.setupResult = response.data.data
        this.setupComplete = true
      } catch (error) {
        console.error('Error submitting setup:', error)
        this.$toast.error('Failed to submit setup: ' + (error.response?.data?.message || error.message))
      }
    },
    copyToClipboard(text, type) {
      navigator.clipboard.writeText(text).then(() => {
        this.copied = type
        setTimeout(() => {
          this.copied = null
        }, 2000)
      })
    }
  }
}
</script>
