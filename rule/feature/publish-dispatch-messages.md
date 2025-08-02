# Feature Overview

A message published to a topic will be stored in the table `message`. And a signal will be sent to all subscribers to start consuming the message.

## Publish Messages

Here are the steps of publishing a message:

1. When a message arrived, we should check if the publisher has the `Write` permission of the topic. 
2. 
