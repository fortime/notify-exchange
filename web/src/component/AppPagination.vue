<template>
  <div class="d-flex justify-content-between align-items-center mt-3">
    <!-- Navigation Buttons -->
    <div class="d-flex align-items-center">
      <BButton
        size="sm"
        :disabled="currentPage === 1 || isLoading"
        @click="changePage(currentPage - 1)"
      >
        Previous
      </BButton>
      <span class="mx-2">Page {{ currentPage }}</span>
      <BButton
        size="sm"
        :disabled="!isNextPageAvailable || isLoading"
        @click="changePage(currentPage + 1)"
      >
        Next
      </BButton>
      <div
        v-if="isLoading"
        class="spinner-border spinner-border-sm ms-2 text-primary"
        role="status"
      >
        <span class="visually-hidden">Loading...</span>
      </div>
    </div>

    <!-- Page Size Selector -->
    <div class="d-flex align-items-center">
      <span class="me-2 text-nowrap">Items per page:</span>
      <BFormSelect
        v-model="perPage"
        :options="perPageOptions"
        size="sm"
        :disabled="isLoading"
        @change="handlePerPageChange"
      />
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, watch } from 'vue'
import { BButton, BFormSelect } from 'bootstrap-vue-next'
import apiClient from '../service/api'
import { useToast } from 'vue-toast-notification'

const props = defineProps({
  url: { type: String, required: true },
  dataKey: { type: String, required: true },
  extraParams: { type: Object, default: () => ({}) },
  initialPerPage: { type: Number, default: 5 },
  perPageOptions: { type: Array, default: () => [5, 10, 25, 50] }
})

const emit = defineEmits(['fetched'])
const $toast = useToast()

const currentPage = ref(1)
const perPage = ref(props.initialPerPage)
const isNextPageAvailable = ref(false)
const isLoading = ref(false)

async function fetchData() {
  if (props.url === "") {
    return
  }
  isLoading.value = true
  try {
    const response = await apiClient.get(props.url, {
      params: {
        page: currentPage.value,
        size: perPage.value,
        ...props.extraParams
      }
    })

    const data = response.data.data
    const items = data[props.dataKey]

    if (data.has_next !== undefined) {
      isNextPageAvailable.value = data.has_next
    } else {
      isNextPageAvailable.value = items.length === perPage.value
    }

    emit('fetched', items, data)
    return data
  } catch (error) {
    const msg = error.response?.data?.message || error.message
    $toast.error(`Failed to fetch ${props.dataKey}: ${msg}`)
    console.error(`[AppPagination] Error fetching ${props.url}:`, error)
  } finally {
    isLoading.value = false
  }
}

function changePage(page) {
  if (page > 0) {
    currentPage.value = page
    fetchData()
  }
}

function handlePerPageChange() {
  currentPage.value = 1
  fetchData()
}

// Re-fetch if extraParams or url change
watch([() => props.extraParams, () => props.url], () => {
  currentPage.value = 1
  fetchData()
}, { deep: true })

onMounted(() => {
  fetchData()
})

// Expose methods for parent components
defineExpose({
  fetchData,
  currentPage,
  perPage
})
</script>
