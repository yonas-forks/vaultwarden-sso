import { test, expect, type TestInfo } from '@playwright/test';
import { MailDev } from 'maildev';

import * as utils from "../global-utils";
import { createAccount, logUser } from './setups/user';

let users = utils.loadEnv();

test.beforeAll('Setup', async ({ browser }, testInfo: TestInfo) => {
    await utils.startVaultwarden(browser, testInfo);
});

test.afterAll('Teardown', async ({}, testInfo: TestInfo) => {
    utils.stopVaultwarden(testInfo);
});

test('Create user3', async ({ page }) => {
    await createAccount(test, page, users.user3);
});

test('Invite users', async ({ page }) => {
    await createAccount(test, page, users.user1);

    await test.step('Create Org', async () => {
        await page.getByRole('link', { name: 'New organisation' }).click();
        await page.getByLabel('Organisation name (required)').fill('Test');
        await page.getByRole('button', { name: 'Submit' }).click();
        await page.locator('div').filter({ hasText: 'Members' }).nth(2).click();
    });

    await test.step('Invite user2', async () => {
        await page.getByRole('button', { name: 'Invite member' }).click();
        await page.getByLabel('Email (required)').fill(users.user2.email);
        await page.getByRole('tab', { name: 'Collections' }).click();
        await page.getByLabel('Permission').selectOption('edit');
        await page.getByLabel('Select collections').click();
        await page.getByLabel('Options list').getByText('Default collection').click();
        await page.getByRole('button', { name: 'Save' }).click();
        await utils.checkNotification(page, 'User(s) invited');
        await expect(page.getByRole('row', { name: users.user2.email })).toHaveText(/Invited/);
    });

    await test.step('Invite user3', async () => {
        await page.getByRole('button', { name: 'Invite member' }).click();
        await page.getByLabel('Email (required)').fill(users.user3.email);
        await page.getByRole('tab', { name: 'Collections' }).click();
        await page.getByLabel('Permission').selectOption('edit');
        await page.getByLabel('Select collections').click();
        await page.getByLabel('Options list').getByText('Default collection').click();
        await page.getByRole('button', { name: 'Save' }).click();
        await utils.checkNotification(page, 'User(s) invited');
        await expect(page.getByRole('row', { name: users.user3.name })).toHaveText(/Needs confirmation/);
    });

    await test.step('Confirm existing user3', async () => {
        await page.getByRole('row', { name: users.user3.name }).getByLabel('Options').click();
        await page.getByRole('menuitem', { name: 'Confirm' }).click();
        await page.getByRole('button', { name: 'Confirm' }).click();
        await utils.checkNotification(page, 'confirmed');
    });
});

test('Create invited account', async ({ page }) => {
    await createAccount(test, page, users.user2);
});

test('Confirm invited user', async ({ page }) => {
    await logUser(test, page, users.user1);
    await page.getByLabel('Switch products').click();
    await page.getByRole('link', { name: ' Admin Console' }).click();
    await page.getByRole('link', { name: 'Members' }).click();

    await test.step('Confirm user2', async () => {
        await page.getByRole('row', { name: users.user2.name }).getByLabel('Options').click();
        await page.getByRole('menuitem', { name: 'Confirm' }).click();
        await page.getByRole('button', { name: 'Confirm' }).click();
        await utils.checkNotification(page, 'confirmed');
    });
});

test('Organization is visible', async ({ context, page }) => {
    await logUser(test, page, users.user2);
    await page.getByLabel('vault: Test').click();
    await expect(page.getByLabel('Filter: Default collection')).toBeVisible();

    const page2 = await context.newPage();
    await logUser(test, page2, users.user3);
    await page2.getByLabel('vault: Test').click();
    await expect(page2.getByLabel('Filter: Default collection')).toBeVisible();
});
