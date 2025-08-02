<template>
  <BContainer>
    <p>{{ message }}</p>
  </BContainer>
</template>

<script setup>
import { onMounted, ref } from 'vue'
import { BContainer } from 'bootstrap-vue-next'

const message = ref("Logging out...")

const loadTelegramSdk = () => {
  if (window.Telegram && window.Telegram.WebApp) {
    window.Telegram.WebApp.close();
  } else {
    const script = document.createElement("script");
    script.src = "https://telegram.org/js/telegram-web-app.js";
    script.onload = () => {
      if (window.Telegram && window.Telegram.WebApp) {
        window.Telegram.WebApp.close();
      } else {
        console.error("Telegram WebApp not available after SDK load.");
        message.value = "Failed to close Telegram Mini App. WebApp not available.";
      }
    };
    script.onerror = () => {
      console.error("Failed to load Telegram SDK.");
      message.value = "Failed to load Telegram SDK. Could not close mini app.";
    };
    document.head.appendChild(script);
  }
};

onMounted(() => {
  loadTelegramSdk();
});
</script>
