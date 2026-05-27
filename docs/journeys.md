# Developer Journeys

Real scenarios — what actually happens at each stage.

---

## Journey 1 — First time opening a project

**Scenario:** You've just cloned a Java microservice you've never seen before. Twelve services, Spring Boot, Maven, no docs.

```bash
cd order-service
zap
```

zap starts in under a second. But the agent has no knowledge of this project yet — it's a blank slate. The right move is `/init`.

```
/init
```

Here's what happens step by step:

```
◌ Detected project type: java
Language(s) for this project: java        ← confirm or correct

◌ Indexing lets zap find symbols instantly without reading every file.
Index this project now? (recommended, ~10s)  Y

  Indexing src/ ...
  ✓ tree-sitter · java · 847 symbols across 63 files

✓ .zap/project.json written.
✓ Created ZAP.md for java project.
⚡ Asking the agent to analyse the repo and fill in ZAP.md…
```

The agent reads the source files and fills in `ZAP.md` — a persistent project knowledge file loaded into every future session:

```markdown
## Overview
Order service — handles order lifecycle (create, fulfil, cancel).
Publishes events to Kafka on state transitions.

## Build & Test
mvn clean install        # full build
mvn test                 # unit tests only
mvn spring-boot:run      # local dev server on :8080

## Architecture
- OrderController  → REST handlers (src/main/java/.../controller/)
- OrderService     → business logic, calls OrderRepository
- OrderRepository  → JPA, Postgres via spring-data
- KafkaProducer    → publishes OrderCreated / OrderFulfilled events

## Important Files
- OrderService.java     — core domain logic, start here
- application.yml       — all config including Kafka brokers
- schema.sql            — DB schema

## Do Not Touch
- LegacyOrderMapper.java — deprecated, kept for backwards compat, do not edit
```

**Total time: ~30 seconds.** From zero to a fully context-aware agent.

---

## Journey 2 — Returning to a project

**Scenario:** You worked on the order service last week. You open zap today to continue.

```bash
cd order-service
zap
```

Cold start. But zap is not starting blind. Before your first message, it has already loaded:

| File | What it contains |
|---|---|
| `ZAP.md` | Project overview, build commands, architecture, do-not-touch list |
| `.zap/understanding.md` | Module map, data flows, patterns, constraints |
| `.zap/context.md` | Last session: goal, files touched, what's next |
| `.zap/session_log.md` | History of all previous sessions |

```
you:  "what were we working on last time?"

zap:  Last session you were adding pagination to GET /orders.
      You updated OrderController.java and OrderService.java.
      The service method was done but the controller test was still failing
      — that was left as the next step.
```

No re-reading files. No re-explaining the stack. The session handoff is automatic.

---

## Journey 3 — Understanding unfamiliar code

**Scenario:** A colleague wrote the `FulfilmentService` six months ago. You need to understand it before touching it.

```
"explain how FulfilmentService works — what it does, what it calls, what could go wrong"
```

```
→ java skill fires (class keyword matched)
→ find_definition looks up FulfilmentService in the index — found at
  src/main/java/.../service/FulfilmentService.java:34
→ code_map outlines all methods: fulfil(), rollback(), notifyWarehouse()
→ reads only the relevant sections, not the whole file
→ traces the call chain: fulfil() → OrderRepository.save() → KafkaProducer.publish()
→ flags: rollback() has no test coverage, notifyWarehouse() blocks on HTTP with no timeout
```

You get a structural explanation in seconds — including the non-obvious risks — without reading 400 lines of Java yourself.

**Drill deeper:**

```
"show me every place FulfilmentService.fulfil() is called"
→ find_references scans the index
→ 3 call sites: OrderController.java:88, OrderSaga.java:142, FulfilmentTest.java:67
```

```
"what does the data flow look like from the REST call to Kafka publish?"
→ traces: POST /fulfil → FulfilmentController → FulfilmentService.fulfil()
         → OrderRepository.save() (Postgres) → KafkaProducer.publish() (Kafka)
→ notes: the Kafka publish happens inside the DB transaction — risky if Kafka is down
```

---

## Journey 4 — Adding a feature to an existing codebase

**Scenario:** You need to add a `GET /orders/{id}/history` endpoint to the existing order service.

Without zap (what most agents do): the agent creates `HistoryController.java`, `HistoryService.java`, `HistoryRepository.java` — duplicating the structure that already exists, ignoring the patterns already in use.

With zap:

```
"add a GET /orders/{id}/history endpoint that returns the state change log"
```

```
→ java skill fires
→ index query: WHERE name LIKE '%Order%' AND kind = 'class'
  finds: OrderController, OrderService, OrderRepository, OrderHistory (entity — already exists!)
→ index query: WHERE name LIKE '%Repository%' AND kind = 'interface'
  finds: OrderRepository extends JpaRepository<Order, Long> — this is the pattern to follow
→ index query: WHERE path LIKE '%controller%' ORDER BY line
  finds: all existing controllers — sees they all extend BaseController, use @RestController

zap:  OrderHistory entity already exists at OrderHistory.java:12.
      I'll add a findByOrderId() method to OrderRepository, a getHistory() method
      to OrderService, and a new endpoint to OrderController — no new files needed.
      Shall I proceed?
```

One file touched in `OrderController`, one method added to `OrderRepository`, one to `OrderService`. No duplicate classes. Follows the existing `JpaRepository` pattern.

**This is the core difference.** The agent knew what existed before it decided what to create.

---

## Journey 5 — Fixing a bug

**Scenario:** Orders are occasionally being marked as fulfilled even when the warehouse notification fails.

```
"orders are being fulfilled even when notifyWarehouse() throws — trace why"
```

```
→ debugging skill fires
→ find_definition locates fulfil() at FulfilmentService.java:67
→ reads the method — sees notifyWarehouse() is called AFTER OrderRepository.save()
→ the save() commits to DB before the warehouse call — if warehouse throws, order
  is already marked fulfilled in the DB with no rollback
→ finds: @Transactional annotation is present but notifyWarehouse() is an HTTP call
  outside the transaction boundary — @Transactional won't roll it back

zap:  Root cause: the DB commit happens before the warehouse HTTP call.
      @Transactional covers the DB write but not the external HTTP call.
      Fix: move notifyWarehouse() before the save(), or wrap it in a try/catch
      that explicitly rolls back via OrderRepository.updateStatus().
      Which approach do you prefer?
```

You pick the approach, zap makes the surgical edit, shows you the diff, asks for approval before writing.

---

## Journey 6 — Checking and improving code quality

**Scenario:** The team suspects the service is getting harder to change. You want data before the next sprint planning.

```
/index quality
```

```
◎ code quality — 63 files · 847 symbols                          score: 61/100
────────────────────────────────────────────────────────────────────────────

⚠ god objects  (impl with >15 methods — split recommended)
  OrderService          34 methods  ██████████████  src/.../service/OrderService.java
  FulfilmentService     18 methods  ███████         src/.../service/FulfilmentService.java

⚠ large files  (>50 symbols)
    91 sym  ████████████████████  OrderService.java
    67 sym  ██████████████        OrderController.java

✦ high coupling  (referenced in many places — risky to change)
  OrderService.fulfil()     29×   FulfilmentService.java:67
  OrderRepository.save()    24×   (multiple callers)

◌ dead code candidates  (public method, 0 external references)
  LegacyOrderMapper.toDto()    LegacyOrderMapper.java:44
  OrderUtils.formatId()        OrderUtils.java:18

→ OrderService has 34 methods — consider splitting into OrderLifecycleService + OrderQueryService
→ 2 public methods never referenced — confirm they can be removed
```

```
"which methods in OrderService are safe to extract to a new OrderQueryService?"
→ queries index for all methods in OrderService
→ cross-references call sites — methods only called from read endpoints are safe to extract
→ lists: findById(), findByStatus(), findByDateRange(), getOrderSummary() — all query-only, no writes
```

---

## Journey 7 — Wrapping up and handing off

At the end of any session:

```
"we added pagination to GET /orders and fixed the fulfilment race condition —
 update context.md with what we did and what's still left"
```

zap writes `.zap/context.md`:

```markdown
## Last updated
2026-05-25 — Session #42

## What was being worked on
Added cursor-based pagination to GET /orders endpoint.
Fixed race condition in FulfilmentService where DB commit preceded warehouse HTTP call.

## Files touched
- OrderController.java
- OrderService.java
- FulfilmentService.java
- OrderControllerTest.java

## What's next
- Pagination test for edge case: empty cursor on last page
- Consider splitting OrderService (34 methods — see /index quality output)
```

Tomorrow's session picks this up automatically. No re-explaining. No lost context.
