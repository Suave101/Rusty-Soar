# Rusty Soar

## Context & Motivation
Next-generation autonomous physical systems—such as Urban Air Mobility (UAM) aircraft must make high-level cognitive decisions in dynamic, unpredictable environments. To operate safely under strict regulatory frameworks, these systems require mathematically provable safety guarantees.

Legacy cognitive architectures, specifically the Soar cognitive engine, offer robust symbolic reasoning based on Rete graph matching and preference resolution. However, existing implementations (primarily Java and C/C++) are fundamentally unsuited for modern safety-critical flight hardware.

## Problem Statement
Current autonomous control architectures suffer from a critical trilemma: they force a choice between real-time determinism, memory safety, and formal certifiability.

Deployed autonomous systems cannot reconcile adaptive neural perception (e.g., Self-Supervised and Continual Learning) with bit-precise symbolic safety bounds, resulting in an "AI Certification Paradox" where advanced cognitive capabilities cannot be certified for flight.

<img width="482" height="482" alt="TriangleOfDispair" src="https://github.com/user-attachments/assets/9a100f4a-bdeb-4c3c-b1c3-61122f48d4b4" />

