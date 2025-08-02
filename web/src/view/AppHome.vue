<template>
  <div class="mt-5">
    <BRow>
      <BCol>
        <TopicsCard class="mb-4" />
      </BCol>
    </BRow>
    <BRow>
      <BCol>
        <EndpointsCard />
      </BCol>
    </BRow>
  </div>
</template>

<script setup>
import { BRow, BCol } from 'bootstrap-vue-next'
import TopicsCard from '../component/topic/TopicsCard.vue'
import EndpointsCard from '../component/endpoint/EndpointsCard.vue'
</script>
